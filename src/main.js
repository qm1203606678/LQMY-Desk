import { createApp } from "vue";
import App from "./App.vue";
import router from "./router";  // 引入路由
import { createPinia } from "pinia";

const app = createApp(App);
const pinia = createPinia();
app.use(router);  // 使用路由
app.use(pinia);
app.mount("#app");
