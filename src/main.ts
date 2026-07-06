import { createApp } from "vue";
import { createPinia } from "pinia";
import ArcoVue from "@arco-design/web-vue";
import App from "./App.vue";
import "@arco-design/web-vue/dist/arco.css";
import "./styles/app.css";

createApp(App).use(createPinia()).use(ArcoVue).mount("#app");
