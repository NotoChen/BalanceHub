import DOMPurify from "dompurify";
import { marked } from "marked";

/// Agent 文档只需要读文字：保留结构，不渲染图片。
const DOCUMENT_TAGS = [
  "p",
  "br",
  "hr",
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "strong",
  "em",
  "del",
  "blockquote",
  "ul",
  "ol",
  "li",
  "pre",
  "code",
  "table",
  "thead",
  "tbody",
  "tr",
  "th",
  "td",
  "a",
  "details",
  "summary",
  "kbd",
];
const DOCUMENT_ATTRIBUTES = ["href", "title", "start", "colspan", "rowspan"];

/// 站点公告由后台人员手写，除了标准 Markdown 还会用 <center> / <span> 做
/// 彩色小标题，并贴维护海报和截图。
const ANNOUNCEMENT_TAGS = [
  ...DOCUMENT_TAGS,
  "img",
  "div",
  "span",
  "u",
  "s",
  "sub",
  "sup",
  "center",
];
const ANNOUNCEMENT_ATTRIBUTES = [
  ...DOCUMENT_ATTRIBUTES,
  "src",
  "alt",
  "width",
  "height",
];

/// 只放行网络图片和常见位图 data URI；SVG 可以内嵌脚本，不当作图片源放行。
const DOCUMENT_URI_PATTERN = /^https?:\/\//i;
const ANNOUNCEMENT_IMAGE_PATTERN =
  /^(?:https?:\/\/|data:image\/(?:png|jpe?g|gif|webp|avif);base64,)/i;

/** Agent 资源文档：只读文本，链接一律交给系统浏览器。 */
export function documentMarkdownToHtml(text: string) {
  return sanitizeMarkdown(text, {
    breaks: false,
    tags: DOCUMENT_TAGS,
    attributes: DOCUMENT_ATTRIBUTES,
    uriPattern: DOCUMENT_URI_PATTERN,
  });
}

/** 站点公告：保留图文混排，图片懒加载且不向图片所在站点透露来源。 */
export function announcementMarkdownToHtml(text: string) {
  return normalizeImages(
    sanitizeMarkdown(text, {
      breaks: true,
      tags: ANNOUNCEMENT_TAGS,
      attributes: ANNOUNCEMENT_ATTRIBUTES,
      uriPattern: ANNOUNCEMENT_IMAGE_PATTERN,
    }),
  );
}

interface SanitizeOptions {
  breaks: boolean;
  tags: string[];
  attributes: string[];
  uriPattern: RegExp;
}

function sanitizeMarkdown(text: string, options: SanitizeOptions) {
  return DOMPurify.sanitize(
    marked.parse(text, { async: false, gfm: true, breaks: options.breaks }),
    {
      ALLOWED_TAGS: options.tags,
      ALLOWED_ATTR: options.attributes,
      ALLOWED_URI_REGEXP: options.uriPattern,
    },
  );
}

/**
 * 公告列表可能同时展开多条，图片不应该在展开瞬间全部下载；
 * 也不把打开公告这件事透露给图片所在的第三方站点。
 *
 * 另外，DOMPurify 对 img 的 data URI 有一条独立放行规则（DATA_URI_TAGS），
 * 会绕过 ALLOWED_URI_REGEXP，所以图片源要在这里按同一份白名单再核一次，
 * 否则 data:image/svg+xml 也能进来。站点常贴 base64 海报，位图必须保留。
 */
function normalizeImages(html: string) {
  const template = document.createElement("template");
  template.innerHTML = html;
  template.content.querySelectorAll("img").forEach((image) => {
    if (!ANNOUNCEMENT_IMAGE_PATTERN.test(image.getAttribute("src")?.trim() ?? "")) {
      image.remove();
      return;
    }
    image.setAttribute("loading", "lazy");
    image.setAttribute("decoding", "async");
    image.setAttribute("referrerpolicy", "no-referrer");
  });
  return template.innerHTML;
}
