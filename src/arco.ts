import { defineAsyncComponent, type App, type Component, type Plugin } from "vue";
import {
  Alert,
  Button,
  ConfigProvider,
  Dropdown,
  Empty,
  Modal,
  Popover,
  Progress,
  Spin,
  Tooltip,
} from "@arco-design/web-vue";

/**
 * 只注册界面实际用到的 Arco 组件，不再整包 `app.use(ArcoVue)`。
 * 每个组件的 install 会一并注册自己的子组件（Button → ButtonGroup、
 * Dropdown → Doption 等），子组件标签不必单独列出。
 *
 * 组件分两档：
 * - `eagerComponents`：首屏就会渲染，入口直接注册，保证输入、弹层没有加载抖动。
 * - `lazyFamilies`：只出现在按需面板里，标签第一次渲染时才异步加载组件与样式，
 *   启动期不再为表单、选择器、抽屉和分页付出解析成本。
 *
 * 模板里新增 `a-*` 标签时，要么加进这两张表，要么在使用它的组件里以
 * `Xxx as AXxx` 本地导入；`tests/arco-registration.test.ts` 会检查每个标签
 * 都有来源。样式：首屏组件在 `arco-styles.ts` 引入，按需组件跟随各自家族加载，
 * 本地注册的组件由使用方引入对应的 `style/css.js`。
 */
const eagerComponents: Plugin[] = [
  Alert,
  Button,
  ConfigProvider,
  Dropdown,
  Empty,
  Modal,
  Popover,
  Progress,
  Spin,
  Tooltip,
];

interface LazyArcoFamily {
  /// 家族主导出；其余名字按同名命名导出取用。
  primary: string;
  /// 需要在全局注册的组件名（含子组件），注册为 `A<name>`。
  names: string[];
  style: () => Promise<unknown>;
  load: () => Promise<unknown>;
}

const lazyFamilies: LazyArcoFamily[] = [
  {
    primary: "Checkbox",
    names: ["Checkbox", "CheckboxGroup"],
    style: () => import("@arco-design/web-vue/es/checkbox/style/css.js"),
    load: () => import("@arco-design/web-vue/es/checkbox"),
  },
  {
    primary: "Collapse",
    names: ["Collapse", "CollapseItem"],
    style: () => import("@arco-design/web-vue/es/collapse/style/css.js"),
    load: () => import("@arco-design/web-vue/es/collapse"),
  },
  {
    primary: "Drawer",
    names: ["Drawer"],
    style: () => import("@arco-design/web-vue/es/drawer/style/css.js"),
    load: () => import("@arco-design/web-vue/es/drawer"),
  },
  {
    primary: "Form",
    names: ["Form", "FormItem"],
    style: () => import("@arco-design/web-vue/es/form/style/css.js"),
    load: () => import("@arco-design/web-vue/es/form"),
  },
  {
    primary: "Input",
    names: ["Input", "InputGroup", "InputSearch", "InputPassword"],
    style: () => import("@arco-design/web-vue/es/input/style/css.js"),
    load: () => import("@arco-design/web-vue/es/input"),
  },
  {
    primary: "InputNumber",
    names: ["InputNumber"],
    style: () => import("@arco-design/web-vue/es/input-number/style/css.js"),
    load: () => import("@arco-design/web-vue/es/input-number"),
  },
  {
    primary: "Pagination",
    names: ["Pagination"],
    style: () => import("@arco-design/web-vue/es/pagination/style/css.js"),
    load: () => import("@arco-design/web-vue/es/pagination"),
  },
  {
    primary: "Popconfirm",
    names: ["Popconfirm"],
    style: () => import("@arco-design/web-vue/es/popconfirm/style/css.js"),
    load: () => import("@arco-design/web-vue/es/popconfirm"),
  },
  {
    primary: "Radio",
    names: ["Radio", "RadioGroup"],
    style: () => import("@arco-design/web-vue/es/radio/style/css.js"),
    load: () => import("@arco-design/web-vue/es/radio"),
  },
  {
    primary: "Select",
    names: ["Select", "Option", "Optgroup"],
    style: () => import("@arco-design/web-vue/es/select/style/css.js"),
    load: () => import("@arco-design/web-vue/es/select"),
  },
  {
    primary: "Skeleton",
    names: ["Skeleton", "SkeletonLine"],
    style: () => import("@arco-design/web-vue/es/skeleton/style/css.js"),
    load: () => import("@arco-design/web-vue/es/skeleton"),
  },
  {
    primary: "Space",
    names: ["Space"],
    style: () => import("@arco-design/web-vue/es/space/style/css.js"),
    load: () => import("@arco-design/web-vue/es/space"),
  },
  {
    primary: "Switch",
    names: ["Switch"],
    style: () => import("@arco-design/web-vue/es/switch/style/css.js"),
    load: () => import("@arco-design/web-vue/es/switch"),
  },
  {
    primary: "Tag",
    names: ["Tag"],
    style: () => import("@arco-design/web-vue/es/tag/style/css.js"),
    load: () => import("@arco-design/web-vue/es/tag"),
  },
  {
    primary: "Textarea",
    names: ["Textarea"],
    style: () => import("@arco-design/web-vue/es/textarea/style/css.js"),
    load: () => import("@arco-design/web-vue/es/textarea"),
  },
];

function lazyArcoComponent(family: LazyArcoFamily, name: string): Component {
  return defineAsyncComponent(async () => {
    const [, loaded] = await Promise.all([family.style(), family.load()]);
    const module = loaded as Record<string, Component>;
    return name === family.primary ? module.default : module[name];
  });
}

export function registerArcoComponents(app: App) {
  for (const component of eagerComponents) {
    app.use(component);
  }
  for (const family of lazyFamilies) {
    for (const name of family.names) {
      app.component(`A${name}`, lazyArcoComponent(family, name));
    }
  }
}
