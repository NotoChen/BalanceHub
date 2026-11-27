/// <reference types="vite/client" />

declare module "*.vue" {
  import type { DefineComponent } from "vue";
  const component: DefineComponent<{}, {}, any>;
  export default component;
}

/// Arco 的按组件样式入口只导入 CSS，没有类型声明。
declare module "@arco-design/web-vue/es/*/style/css.js";
