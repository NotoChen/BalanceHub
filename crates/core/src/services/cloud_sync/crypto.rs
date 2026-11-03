use super::format::{Head, Manifest, FORMAT, VERSION};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD, Engine};
use chacha20poly1305::{
    aead::{Aead, Payload},
    KeyInit, XChaCha20Poly1305, XNonce,
};

pub(super) struct Cipher {
    cipher: XChaCha20Poly1305,
    pub salt: String,
}

impl Cipher {
    pub fn derive(passphrase: &str, salt: Option<&str>) -> Result<Self, String> {
        let salt = match salt {
            Some(value) => STANDARD.decode(value).map_err(|_| "云端加密参数无效")?,
            None => {
                let mut value = [0; 16];
                getrandom::fill(&mut value).map_err(|_| "无法生成加密参数")?;
                value.to_vec()
            }
        };
        if salt.len() != 16 {
            return Err("云端加密参数无效".to_owned());
        }
        let mut key = [0; 32];
        let params = Params::new(19 * 1024, 2, 1, Some(32)).map_err(|_| "加密参数无效")?;
        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password_into(passphrase.as_bytes(), &salt, &mut key)
            .map_err(|_| "无法生成同步密钥")?;
        let cipher = XChaCha20Poly1305::new((&key).into());
        key.fill(0);
        Ok(Self {
            cipher,
            salt: STANDARD.encode(salt),
        })
    }

    pub fn encrypt(&self, bytes: &[u8], context: &[u8]) -> Result<Vec<u8>, String> {
        let mut nonce = [0; 24];
        getrandom::fill(&mut nonce).map_err(|_| "无法生成加密随机数")?;
        let ciphertext = self
            .cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: bytes,
                    aad: context,
                },
            )
            .map_err(|_| "同步数据加密失败")?;
        let mut output = nonce.to_vec();
        output.extend_from_slice(&ciphertext);
        Ok(output)
    }

    pub fn decrypt(&self, bytes: &[u8], context: &[u8]) -> Result<Vec<u8>, String> {
        if bytes.len() < 40 {
            return Err("云端加密文件不完整".to_owned());
        }
        self.cipher
            .decrypt(
                XNonce::from_slice(&bytes[..24]),
                Payload {
                    msg: &bytes[24..],
                    aad: context,
                },
            )
            .map_err(|_| "解密失败：同步密码不正确，或云端文件已损坏".to_owned())
    }

    pub fn encode_head(&self, manifest: &Manifest) -> Result<Vec<u8>, String> {
        let plain = serde_json::to_vec(manifest).map_err(|_| "同步清单无法编码")?;
        let head = Head {
            format: FORMAT.to_owned(),
            version: VERSION,
            salt: self.salt.clone(),
            manifest: STANDARD.encode(self.encrypt(&plain, b"balancehub-manifest-v1")?),
        };
        serde_json::to_vec(&head).map_err(|_| "同步清单无法编码".to_owned())
    }

    pub fn decode_head(&self, head: &Head) -> Result<Manifest, String> {
        let bytes = STANDARD
            .decode(&head.manifest)
            .map_err(|_| "云端清单损坏")?;
        let decoded = self.decrypt(&bytes, b"balancehub-manifest-v1")?;
        let manifest: Manifest =
            serde_json::from_slice(&decoded).map_err(|_| "云端清单格式无效")?;
        if manifest.version != VERSION || manifest.entries.len() > 100_000 {
            return Err("云端同步版本不支持或清单过大，请升级 BalanceHub 后重试".to_owned());
        }
        Ok(manifest)
    }
}

pub(super) fn parse_head(bytes: &[u8]) -> Result<Head, String> {
    let head: Head = serde_json::from_slice(bytes).map_err(|_| "云端同步清单格式无效")?;
    if head.format != FORMAT || head.version != VERSION {
        return Err("此目录中的同步数据版本不兼容，请升级 BalanceHub 或选择独立目录".to_owned());
    }
    Ok(head)
}
