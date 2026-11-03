/** 浏览器版下载：把文本交给当前浏览器保存，不经过服务端文件路径。 */
export function downloadTextFile(
  fileName: string,
  text: string,
  type = "application/json",
) {
  const blob = new Blob([text], { type });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = fileName;
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 10_000);
}
