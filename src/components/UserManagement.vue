<template>
    <div>
        <h1>用户管理</h1>
        <input type="text" v-model="searchQuery" placeholder="搜索设备名或序列号..." />
        <table>
            <thead>
                <tr>
                    <th>设备名</th>
                    <th>设备序列号</th>
                    <th>用户类别</th>
                    <th>操作</th>
                </tr>
            </thead>
            <tbody>
                <tr v-for="user in filteredUsers" :key="user.serial">
                    <td>{{ user.name }}</td>
                    <td>{{ user.serial }}</td>
                    <td>
                        <select v-model="user.category">
                            <option value="trusted">可信</option>
                            <option value="regular">普通</option>
                            <option value="blacklist">黑名单</option>
                        </select>
                    </td>
                    <td>
                        <button @click="updateUser(user)">更改类别</button>
                        <button @click="deleteUser(user.serial)">删除</button>
                    </td>
                </tr>
            </tbody>
        </table>
    </div>
</template>

<script>
import { ref, computed, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";

export default {
    setup() {
        const users = ref([]);
        const searchQuery = ref("");

        const filteredUsers = computed(() => {
            return users.value.filter(user =>
                user.name.includes(searchQuery.value) || user.serial.includes(searchQuery.value)
            );
        });

        async function fetchUsers() {
            try {
                users.value = await invoke("get_users");
            } catch (error) {
                console.error("获取用户列表失败:", error);
            }
        }

        async function updateUser(user) {
            try {
                await invoke("update_user_category", { serial: user.serial, category: user.category });
                alert("用户类别更新成功");
            } catch (error) {
                console.error("更新用户类别失败:", error);
            }
        }

        async function deleteUser(serial) {
            if (confirm("确定删除该用户？")) {
                try {
                    await invoke("delete_user", { serial });
                    users.value = users.value.filter(u => u.serial !== serial);
                } catch (error) {
                    console.error("删除用户失败:", error);
                }
            }
        }

        onMounted(fetchUsers);

        return { searchQuery, filteredUsers, updateUser, deleteUser };
    }
};
</script>

<style scoped>
input {
    width: 300px;
    padding: 8px;
    margin-bottom: 10px;
}

table {
    width: 100%;
    border-collapse: collapse;
}

th,
td {
    border: 1px solid #ddd;
    padding: 8px;
    text-align: center;
}
</style>