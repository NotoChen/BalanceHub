import { createApp } from "vue";
import { createPinia } from "pinia";
import ArcoVue, { Modal } from "@arco-design/web-vue";
import App from "./App.vue";
import "@arco-design/web-vue/dist/arco.css";
import "./styles/app.css";

const app = createApp(App);
app.use(createPinia()).use(ArcoVue);
// 静态弹窗需要继承应用的全局组件注册和注入依赖。
Modal._context = app._context;
app.mount("#app");
