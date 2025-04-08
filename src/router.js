import { createRouter, createWebHistory } from "vue-router";
import HomeView from "./components/HomeView.vue";
import UserManagement from "./components/UserManagement.vue";

const routes = [
  { path: "/", component: HomeView },
  { path: "/users", component: UserManagement }
];

const router = createRouter({
  history: createWebHistory(),
  routes
});

export default router;
