use super::{DirectoryEntryCandidate, DirectoryLinkEvidence, VerifiedPathError};
use crate::models::AgentAssetSourceKind;
use sha2::{Digest, Sha256};
use std::{
    ffi::{OsStr, OsString},
    fs::{File, OpenOptions},
    io,
    mem::{offset_of, size_of, size_of_val},
    os::windows::{ffi::OsStringExt, fs::OpenOptionsExt, io::AsRawHandle},
    path::{Component, Path, PathBuf, Prefix},
};
use windows_sys::Win32::{
    Foundation::ERROR_NO_MORE_FILES,
    Storage::FileSystem::{
        FileAttributeTagInfo, FileBasicInfo, FileIdBothDirectoryInfo,
        FileIdBothDirectoryRestartInfo, FileIdInfo, GetFileInformationByHandleEx,
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO,
        FILE_BASIC_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_ID_BOTH_DIR_INFO, FILE_ID_INFO, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ, FILE_SHARE_WRITE,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ObjectIdentity {
    volume_serial_number: u64,
    file_id: [u8; 16],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ObjectStamp {
    identity: ObjectIdentity,
    kind: AgentAssetSourceKind,
    attributes: u32,
    reparse_tag: u32,
    size: u64,
    creation_time: i64,
    last_write_time: i64,
    change_time: i64,
}

impl ObjectStamp {
    fn read(handle: &File) -> Result<Self, VerifiedPathError> {
        let identity = query_handle_information::<FILE_ID_INFO>(handle, FileIdInfo)?;
        let attributes =
            query_handle_information::<FILE_ATTRIBUTE_TAG_INFO>(handle, FileAttributeTagInfo)?;
        if attributes.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(VerifiedPathError::SymlinkRejected);
        }
        let basic = query_handle_information::<FILE_BASIC_INFO>(handle, FileBasicInfo)?;
        let metadata = handle.metadata()?;
        let kind = if metadata.is_dir() {
            AgentAssetSourceKind::Directory
        } else if metadata.is_file() {
            AgentAssetSourceKind::File
        } else {
            return Err(VerifiedPathError::SourceChanged);
        };
        Ok(Self {
            identity: ObjectIdentity {
                volume_serial_number: identity.VolumeSerialNumber,
                file_id: identity.FileId.Identifier,
            },
            kind,
            attributes: attributes.FileAttributes,
            reparse_tag: attributes.ReparseTag,
            size: metadata.len(),
            creation_time: basic.CreationTime,
            last_write_time: basic.LastWriteTime,
            change_time: basic.ChangeTime,
        })
    }

    fn same_object(self, other: Self) -> bool {
        self.identity == other.identity
            && self.kind == other.kind
            && self.attributes == other.attributes
            && self.reparse_tag == other.reparse_tag
    }

    fn update_identity(self, hasher: &mut Sha256) {
        hasher.update(self.identity.volume_serial_number.to_le_bytes());
        hasher.update(self.identity.file_id);
        hasher.update([self.kind as u8]);
        hasher.update(self.attributes.to_le_bytes());
        hasher.update(self.reparse_tag.to_le_bytes());
    }

    fn update_revision(self, hasher: &mut Sha256) {
        self.update_identity(hasher);
        hasher.update(self.size.to_le_bytes());
        hasher.update(self.creation_time.to_le_bytes());
        hasher.update(self.last_write_time.to_le_bytes());
        hasher.update(self.change_time.to_le_bytes());
    }
}

struct PathNode {
    handle: File,
    stamp: ObjectStamp,
}

#[derive(Debug, Clone)]
pub(super) struct PlatformAnchor {
    stamps: Vec<ObjectStamp>,
    allowed_root_index: usize,
}

pub(super) struct PlatformGuard {
    nodes: Vec<PathNode>,
    allowed_root_index: usize,
}

impl PlatformGuard {
    pub(super) fn open(
        path: &Path,
        allowed_root: &Path,
        kind: AgentAssetSourceKind,
    ) -> Result<Self, VerifiedPathError> {
        let component_paths = verified_component_paths(path)?;
        let allowed_root_index = component_paths
            .iter()
            .position(|path| path == allowed_root)
            .ok_or(VerifiedPathError::OutsideAllowedRoot)?;
        let mut nodes = Vec::with_capacity(component_paths.len());
        for (index, component_path) in component_paths.iter().enumerate() {
            let handle = open_without_delete_share(component_path)?;
            let stamp = ObjectStamp::read(&handle)?;
            let expected_kind = if index + 1 == component_paths.len() {
                kind
            } else {
                AgentAssetSourceKind::Directory
            };
            if stamp.kind != expected_kind {
                return Err(VerifiedPathError::TypeMismatch { actual: stamp.kind });
            }
            nodes.push(PathNode { handle, stamp });
        }
        Ok(Self {
            nodes,
            allowed_root_index,
        })
    }

    pub(super) fn source_handle(&self) -> &File {
        &self.nodes.last().expect("verified source handle").handle
    }

    pub(super) fn read_entries(&self) -> io::Result<DirectoryReader<'_>> {
        Ok(DirectoryReader {
            handle: self.source_handle(),
            buffer: vec![0_u64; DirectoryReader::BUFFER_BYTES / size_of::<u64>()]
                .into_boxed_slice(),
            next_offset: None,
            needs_refill: true,
            first_refill: true,
            finished: false,
        })
    }

    pub(super) fn read_directory_link(
        &self,
        _entry_name: &OsStr,
    ) -> Result<DirectoryLinkEvidence, VerifiedPathError> {
        // Reparse-point traversal has no validated native reference mechanism.
        Err(VerifiedPathError::SymlinkRejected)
    }

    pub(super) fn revalidate(&self) -> Result<(), VerifiedPathError> {
        self.revalidate_identity()?;
        let source = self.nodes.last().expect("verified source handle");
        if ObjectStamp::read(&source.handle)? != source.stamp {
            return Err(VerifiedPathError::SourceChanged);
        }
        Ok(())
    }

    pub(super) fn revalidate_identity(&self) -> Result<(), VerifiedPathError> {
        for (index, node) in self.nodes.iter().enumerate() {
            let current = ObjectStamp::read(&node.handle)?;
            if !current.same_object(node.stamp) {
                return Err(self.changed(index));
            }
        }
        Ok(())
    }

    pub(super) fn anchor(&self) -> PlatformAnchor {
        PlatformAnchor {
            stamps: self.nodes.iter().map(|node| node.stamp).collect(),
            allowed_root_index: self.allowed_root_index,
        }
    }

    pub(super) fn compare_anchor(
        &self,
        expected: &PlatformAnchor,
    ) -> Result<(), VerifiedPathError> {
        self.compare_anchor_identity(expected)?;
        if self.nodes.last().expect("verified source handle").stamp
            != *expected.stamps.last().expect("source stamp")
        {
            return Err(VerifiedPathError::SourceChanged);
        }
        Ok(())
    }

    pub(super) fn compare_anchor_identity(
        &self,
        expected: &PlatformAnchor,
    ) -> Result<(), VerifiedPathError> {
        if self.nodes.len() != expected.stamps.len()
            || self.allowed_root_index != expected.allowed_root_index
        {
            return Err(VerifiedPathError::RootChanged);
        }
        for (index, (node, expected)) in self.nodes.iter().zip(&expected.stamps).enumerate() {
            if !node.stamp.same_object(*expected) {
                return Err(self.changed(index));
            }
        }
        Ok(())
    }

    pub(super) fn update_revision(&self, hasher: &mut Sha256) {
        hasher.update(b"windows-verified-path-v1");
        hasher.update((self.allowed_root_index as u64).to_le_bytes());
        for node in &self.nodes {
            node.stamp.update_identity(hasher);
        }
        self.nodes
            .last()
            .expect("verified source handle")
            .stamp
            .update_revision(hasher);
    }

    fn changed(&self, index: usize) -> VerifiedPathError {
        if index <= self.allowed_root_index {
            VerifiedPathError::RootChanged
        } else {
            VerifiedPathError::SourceChanged
        }
    }
}

fn verified_component_paths(path: &Path) -> io::Result<Vec<PathBuf>> {
    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    };
    if !matches!(
        prefix.kind(),
        Prefix::Disk(_) | Prefix::VerbatimDisk(_) | Prefix::UNC(_, _) | Prefix::VerbatimUNC(_, _)
    ) {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    let Some(Component::RootDir) = components.next() else {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    };
    let mut current = PathBuf::from(prefix.as_os_str());
    current.push(Path::new(r"\"));
    let mut paths = vec![current.clone()];
    for component in components {
        match component {
            Component::Normal(component) => {
                current.push(component);
                paths.push(current.clone());
            }
            Component::CurDir => {}
            Component::ParentDir | Component::Prefix(_) | Component::RootDir => {
                return Err(io::Error::from(io::ErrorKind::InvalidInput));
            }
        }
    }
    Ok(paths)
}

fn open_without_delete_share(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    // FILE_LIST_DIRECTORY and FILE_READ_DATA share the Win32 access bit.
    // FILE_SHARE_DELETE is deliberately absent on ancestors and final files.
    options
        .read(true)
        .access_mode(FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

fn query_handle_information<T: Default>(handle: &File, class: i32) -> io::Result<T> {
    let mut information = T::default();
    let size =
        u32::try_from(size_of::<T>()).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    // SAFETY: the live handle and writable exact-size structure match class.
    let succeeded = unsafe {
        GetFileInformationByHandleEx(
            handle.as_raw_handle(),
            class,
            (&mut information as *mut T).cast(),
            size,
        )
    };
    if succeeded == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(information)
}

struct ParsedDirectoryRecord {
    entry: DirectoryEntryCandidate,
    next_offset: Option<usize>,
}

fn parse_directory_record(buffer: &[u64], offset: usize) -> io::Result<ParsedDirectoryRecord> {
    let buffer_bytes = size_of_val(buffer);
    let header_bytes = offset_of!(FILE_ID_BOTH_DIR_INFO, FileName);
    let last_header_offset = buffer_bytes
        .checked_sub(header_bytes)
        .ok_or_else(invalid_directory_record)?;
    if offset % size_of::<u64>() != 0 || offset > last_header_offset {
        return Err(invalid_directory_record());
    }

    // SAFETY: a `[u64]` allocation is valid to inspect as the same number of
    // bytes. The returned slice cannot outlive `buffer`, and all field reads
    // below copy from bounds-checked byte ranges.
    let bytes =
        unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), size_of_val(buffer)) };
    let field_offset = |field: usize| {
        offset
            .checked_add(field)
            .ok_or_else(invalid_directory_record)
    };
    let next_entry_offset = read_u32(
        bytes,
        field_offset(offset_of!(FILE_ID_BOTH_DIR_INFO, NextEntryOffset))?,
    )?;
    let file_attributes = read_u32(
        bytes,
        field_offset(offset_of!(FILE_ID_BOTH_DIR_INFO, FileAttributes))?,
    )?;
    let name_bytes = read_u32(
        bytes,
        field_offset(offset_of!(FILE_ID_BOTH_DIR_INFO, FileNameLength))?,
    )? as usize;
    if name_bytes == 0 || name_bytes % size_of::<u16>() != 0 {
        return Err(invalid_directory_record());
    }

    let name_offset = offset
        .checked_add(header_bytes)
        .ok_or_else(invalid_directory_record)?;
    let name_end = name_offset
        .checked_add(name_bytes)
        .ok_or_else(invalid_directory_record)?;
    let name_slice = bytes
        .get(name_offset..name_end)
        .ok_or_else(invalid_directory_record)?;
    let name_units = name_slice
        .chunks_exact(size_of::<u16>())
        .map(|unit| u16::from_ne_bytes([unit[0], unit[1]]))
        .collect::<Vec<_>>();

    let next_offset = if next_entry_offset == 0 {
        None
    } else {
        let next_delta = next_entry_offset as usize;
        let record_bytes = header_bytes
            .checked_add(name_bytes)
            .ok_or_else(invalid_directory_record)?;
        let next_offset = offset
            .checked_add(next_delta)
            .ok_or_else(invalid_directory_record)?;
        if next_delta % size_of::<u64>() != 0
            || next_delta < record_bytes
            || next_offset > last_header_offset
        {
            return Err(invalid_directory_record());
        }
        Some(next_offset)
    };

    Ok(ParsedDirectoryRecord {
        entry: DirectoryEntryCandidate {
            name: OsString::from_wide(&name_units),
            source_kind: if file_attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
                AgentAssetSourceKind::Directory
            } else {
                AgentAssetSourceKind::File
            },
            is_symlink: file_attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0,
        },
        next_offset,
    })
}

fn read_u32(bytes: &[u8], offset: usize) -> io::Result<u32> {
    let end = offset
        .checked_add(size_of::<u32>())
        .ok_or_else(invalid_directory_record)?;
    let field = bytes
        .get(offset..end)
        .ok_or_else(invalid_directory_record)?;
    Ok(u32::from_ne_bytes(
        field
            .try_into()
            .expect("validated u32 directory record field"),
    ))
}

fn invalid_directory_record() -> io::Error {
    io::Error::from(io::ErrorKind::InvalidData)
}

pub(crate) struct DirectoryReader<'a> {
    handle: &'a File,
    buffer: Box<[u64]>,
    next_offset: Option<usize>,
    needs_refill: bool,
    first_refill: bool,
    finished: bool,
}

impl DirectoryReader<'_> {
    const BUFFER_BYTES: usize = 64 * 1024;

    fn refill(&mut self) -> io::Result<bool> {
        self.buffer.fill(0);
        let class = if self.first_refill {
            FileIdBothDirectoryRestartInfo
        } else {
            FileIdBothDirectoryInfo
        };
        self.first_refill = false;
        // SAFETY: the handle remains alive for the reader lifetime and the
        // eight-byte-aligned buffer is writable for the supplied byte length.
        let succeeded = unsafe {
            GetFileInformationByHandleEx(
                self.handle.as_raw_handle(),
                class,
                self.buffer.as_mut_ptr().cast(),
                u32::try_from(self.buffer.len() * size_of::<u64>())
                    .expect("directory buffer length fits u32"),
            )
        };
        if succeeded == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                self.finished = true;
                self.next_offset = None;
                return Ok(false);
            }
            self.finished = true;
            return Err(error);
        }
        self.next_offset = Some(0);
        self.needs_refill = false;
        Ok(true)
    }

    fn read_current(&mut self) -> io::Result<DirectoryEntryCandidate> {
        let Some(offset) = self.next_offset else {
            return self.invalid_data();
        };
        let parsed = match parse_directory_record(&self.buffer, offset) {
            Ok(parsed) => parsed,
            Err(error) => {
                self.finished = true;
                self.next_offset = None;
                self.needs_refill = false;
                return Err(error);
            }
        };
        if parsed.next_offset.is_none() {
            self.next_offset = None;
            self.needs_refill = true;
        } else {
            self.next_offset = parsed.next_offset;
        }
        Ok(parsed.entry)
    }

    fn invalid_data<T>(&mut self) -> io::Result<T> {
        self.finished = true;
        Err(io::Error::from(io::ErrorKind::InvalidData))
    }
}

impl Iterator for DirectoryReader<'_> {
    type Item = io::Result<DirectoryEntryCandidate>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        loop {
            if self.needs_refill {
                match self.refill() {
                    Ok(true) => {}
                    Ok(false) => return None,
                    Err(error) => return Some(Err(error)),
                }
            }
            match self.read_current() {
                Ok(entry) if entry.name == "." || entry.name == ".." => continue,
                result => return Some(result),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn aligned_buffer(byte_len: usize) -> Box<[u64]> {
        vec![0_u64; byte_len.div_ceil(size_of::<u64>())].into_boxed_slice()
    }

    fn buffer_bytes_mut(buffer: &mut [u64]) -> &mut [u8] {
        // SAFETY: the byte slice aliases the unique mutable `u64` slice for
        // exactly its initialized allocation size and does not escape it.
        let byte_len = size_of_val(buffer);
        let pointer = buffer.as_mut_ptr().cast::<u8>();
        unsafe { std::slice::from_raw_parts_mut(pointer, byte_len) }
    }

    fn set_u32(buffer: &mut [u64], offset: usize, value: u32) {
        buffer_bytes_mut(buffer)[offset..offset + size_of::<u32>()]
            .copy_from_slice(&value.to_ne_bytes());
    }

    fn record_buffer(name: &str, next_entry_offset: u32, minimum_bytes: usize) -> Box<[u64]> {
        let header_bytes = offset_of!(FILE_ID_BOTH_DIR_INFO, FileName);
        let name_units = name.encode_utf16().collect::<Vec<_>>();
        let name_bytes = name_units.len() * size_of::<u16>();
        let mut buffer = aligned_buffer(minimum_bytes.max(header_bytes + name_bytes));
        set_u32(
            &mut buffer,
            offset_of!(FILE_ID_BOTH_DIR_INFO, NextEntryOffset),
            next_entry_offset,
        );
        set_u32(
            &mut buffer,
            offset_of!(FILE_ID_BOTH_DIR_INFO, FileAttributes),
            FILE_ATTRIBUTE_DIRECTORY,
        );
        set_u32(
            &mut buffer,
            offset_of!(FILE_ID_BOTH_DIR_INFO, FileNameLength),
            u32::try_from(name_bytes).unwrap(),
        );
        let bytes = buffer_bytes_mut(&mut buffer);
        for (index, unit) in name_units.into_iter().enumerate() {
            let start = header_bytes + index * size_of::<u16>();
            bytes[start..start + size_of::<u16>()].copy_from_slice(&unit.to_ne_bytes());
        }
        buffer
    }

    fn assert_invalid(buffer: &[u64]) {
        let error = parse_directory_record(buffer, 0)
            .err()
            .expect("malformed record must fail");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn directory_record_parser_accepts_an_owned_entry_and_next_cursor() {
        let header_bytes = offset_of!(FILE_ID_BOTH_DIR_INFO, FileName);
        let next_offset = (header_bytes + 2).next_multiple_of(8);
        let buffer = record_buffer("a", next_offset as u32, next_offset + header_bytes);

        let parsed = parse_directory_record(&buffer, 0).unwrap();
        assert_eq!(parsed.entry.name, "a");
        assert_eq!(parsed.entry.source_kind, AgentAssetSourceKind::Directory);
        assert!(!parsed.entry.is_symlink);
        assert_eq!(parsed.next_offset, Some(next_offset));
    }

    #[test]
    fn directory_record_parser_rejects_malformed_variable_records() {
        let header_bytes = offset_of!(FILE_ID_BOTH_DIR_INFO, FileName);
        let name_length_offset = offset_of!(FILE_ID_BOTH_DIR_INFO, FileNameLength);
        let next_offset_field = offset_of!(FILE_ID_BOTH_DIR_INFO, NextEntryOffset);
        let record_bytes = header_bytes + size_of::<u16>();
        let aligned_record_bytes = record_bytes.next_multiple_of(8);

        let mut odd_name = record_buffer("a", 0, record_bytes);
        set_u32(&mut odd_name, name_length_offset, 1);
        assert_invalid(&odd_name);

        let mut name_out_of_bounds = record_buffer("a", 0, record_bytes);
        let oversized_name_bytes = name_out_of_bounds.len() * size_of::<u64>() - header_bytes + 2;
        set_u32(
            &mut name_out_of_bounds,
            name_length_offset,
            u32::try_from(oversized_name_bytes).unwrap(),
        );
        assert_invalid(&name_out_of_bounds);

        let mut unaligned_next = record_buffer(
            "a",
            (aligned_record_bytes + 1) as u32,
            aligned_record_bytes + 1 + header_bytes,
        );
        set_u32(
            &mut unaligned_next,
            next_offset_field,
            (aligned_record_bytes + 1) as u32,
        );
        assert_invalid(&unaligned_next);

        let mut overlapping_next = record_buffer(
            "a",
            (aligned_record_bytes - 8) as u32,
            aligned_record_bytes + header_bytes,
        );
        set_u32(
            &mut overlapping_next,
            next_offset_field,
            (aligned_record_bytes - 8) as u32,
        );
        assert_invalid(&overlapping_next);

        let next_out_of_bounds =
            record_buffer("a", aligned_record_bytes as u32, aligned_record_bytes);
        assert_invalid(&next_out_of_bounds);

        let mut empty_name = record_buffer("a", 0, record_bytes);
        set_u32(&mut empty_name, name_length_offset, 0);
        assert_invalid(&empty_name);
    }

    #[test]
    fn directory_reader_is_fused_after_a_parser_error() {
        let header_bytes = offset_of!(FILE_ID_BOTH_DIR_INFO, FileName);
        let mut buffer = record_buffer("a", 0, header_bytes + size_of::<u16>());
        set_u32(
            &mut buffer,
            offset_of!(FILE_ID_BOTH_DIR_INFO, FileNameLength),
            1,
        );
        let handle = File::open(std::env::current_exe().unwrap()).unwrap();
        let mut reader = DirectoryReader {
            handle: &handle,
            buffer,
            next_offset: Some(0),
            needs_refill: false,
            first_refill: false,
            finished: false,
        };

        assert_eq!(
            reader.next().unwrap().err().unwrap().kind(),
            io::ErrorKind::InvalidData
        );
        assert!(reader.next().is_none());
    }

    #[test]
    fn verified_component_paths_accept_only_supported_absolute_prefixes() {
        for (input, expected) in [
            (
                r"C:\Users\alice",
                vec![r"C:\", r"C:\Users", r"C:\Users\alice"],
            ),
            (
                r"\\server\share\folder",
                vec![r"\\server\share\", r"\\server\share\folder"],
            ),
            (
                r"\\?\C:\Users\alice",
                vec![r"\\?\C:\", r"\\?\C:\Users", r"\\?\C:\Users\alice"],
            ),
            (
                r"\\?\UNC\server\share\folder",
                vec![r"\\?\UNC\server\share\", r"\\?\UNC\server\share\folder"],
            ),
        ] {
            assert_eq!(
                verified_component_paths(Path::new(input)).unwrap(),
                expected.into_iter().map(PathBuf::from).collect::<Vec<_>>()
            );
        }

        for invalid in [
            r"relative\folder",
            r"C:relative\folder",
            r"\rooted\without\drive",
            r"C:\safe\..\outside",
            r"\\.\PIPE\balancehub",
        ] {
            assert!(verified_component_paths(Path::new(invalid)).is_err());
        }
    }

    #[test]
    fn verified_directory_rejects_final_and_ancestor_directory_symlinks() {
        use std::os::windows::fs::symlink_dir;

        let fixture = std::env::temp_dir().join(format!(
            "balancehub-windows-reparse-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let actual = fixture.join("actual");
        let actual_source = actual.join("source");
        let final_link = fixture.join("final-link");
        let ancestor_link = fixture.join("ancestor-link");
        std::fs::create_dir_all(&actual_source).unwrap();
        symlink_dir(&actual_source, &final_link)
            .expect("Windows native test runner must permit directory symlink fixtures");
        symlink_dir(&actual, &ancestor_link)
            .expect("Windows native test runner must permit directory symlink fixtures");

        for path in [&final_link, &ancestor_link.join("source")] {
            assert!(matches!(
                PlatformGuard::open(path, path, AgentAssetSourceKind::Directory).err(),
                Some(VerifiedPathError::SymlinkRejected)
            ));
        }

        std::fs::remove_dir(&final_link).unwrap();
        std::fs::remove_dir(&ancestor_link).unwrap();
        std::fs::remove_dir_all(fixture).unwrap();
    }
}
