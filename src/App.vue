<template>
  <div id="app">
    <nav>
      <router-link to="/">服务器状态</router-link>
      <router-link to="/users">用户管理</router-link>
    </nav>
    <router-view />
  </div>
</template>

<script>
export default {
  setup() { }
};
</script>

<style>
#app {
  font-family: Arial, sans-serif;
  text-align: center;
  margin-top: 20px;
}

nav {
  display: flex;
  justify-content: center;
  gap: 20px;
  margin-bottom: 20px;
}

nav a {
  text-decoration: none;
  padding: 10px 15px;
  border-radius: 5px;
  background: #007bff;
  color: white;
}

nav a.router-link-exact-active {
  background: #0056b3;
}
</style>
