/**
 * 首屏组件的 Arco 样式，替代整包 `dist/arco.css`。
 *
 * 这里只列 `arco.ts` 中 `eagerComponents` 用到的样式；只出现在按需面板里的
 * 组件（表单、选择器、抽屉、分页等）由 `lazyFamilies` 在标签首次渲染时
 * 连同组件一起加载，对应关系在 `arco.ts` 中维护。
 *
 * 本地注册的组件（DatePicker、TimePicker、InputTag、Tabs、List）由使用方
 * 自行引入 `style/css.js`。`tests/arco-registration.test.ts` 会检查三处一致。
 */
import "@arco-design/web-vue/es/alert/style/css.js";
import "@arco-design/web-vue/es/button/style/css.js";
import "@arco-design/web-vue/es/config-provider/style/css.js";
import "@arco-design/web-vue/es/dropdown/style/css.js";
import "@arco-design/web-vue/es/empty/style/css.js";
import "@arco-design/web-vue/es/modal/style/css.js";
import "@arco-design/web-vue/es/popover/style/css.js";
import "@arco-design/web-vue/es/progress/style/css.js";
import "@arco-design/web-vue/es/spin/style/css.js";
import "@arco-design/web-vue/es/tooltip/style/css.js";
// Message 只通过 JS API 调用，没有模板标签，样式需要单独引入。
import "@arco-design/web-vue/es/message/style/css.js";
