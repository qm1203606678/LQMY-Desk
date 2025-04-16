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
                <tr v-for="user in filteredUsers" :key="user.device_id">
                    <td>{{ user.device_name }}</td>
                    <td>{{ user.device_id }}</td>
                    <td>{{ formatUserType(user.user_type) }}</td>
                    <td>
                        <div v-if="editingUserId === user.device_id">
                            <select @change="selectCategory(user, $event)">
                                <option disabled selected value="">请选择新类别</option>
                                <option v-for="type in availableCategories(user.user_type)" :key="type" :value="type">
                                    {{ formatUserType(type) }}
                                </option>
                            </select>
                        </div>
                        <div v-else>
                            <button @click="startEditing(user.device_id)">更改类别</button>
                            <button @click="deleteUser(user.device_id)">删除</button>
                        </div>
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

        const editingUserId = ref(null); // 正在编辑的 user.device_id

        const userTypeLabels = {
            trusted: "可信",
            regular: "普通",
            blacklist: "黑名单"
        };

        const formatUserType = (type) => {
            return userTypeLabels[type] || "未知";
        };

        const availableCategories = (currentType) => {
            return Object.keys(userTypeLabels).filter((t) => t !== currentType);
        };

        function startEditing(deviceId) {
            editingUserId.value = deviceId;
        }

        async function selectCategory(user, event) {
            const newType = event.target.value;
            if (!newType || newType === user.user_type) {
                return; // 未选择或没变更就不处理
            }

            try {
                await invoke("update_user_type", {
                    serial: user.device_id,
                    usertype: newType
                });
                user.user_type = newType;
                editingUserId.value = null;
                alert("用户类别更新成功");
            } catch (error) {
                console.error("更新用户类别失败:", error);
            }
        }


        const filteredUsers = computed(() => {
            return users.value.filter(user =>
                user.device_name?.includes(searchQuery.value) || user.device_id?.includes(searchQuery.value)
            );
        });

        async function fetchUsers() {
            try {
                users.value = await invoke("get_user_info");
                console.log("成功获取用户信息:", users.value);
            } catch (error) {
                console.error("获取用户列表失败:", error);
            }
        }

        async function updateUser(user) {
            try {
                await invoke("update_user_type", { serial: user.device_id, category: user.user_type });
                alert("用户类别更新成功");
            } catch (error) {
                console.error("更新用户类别失败:", error);
            }
        }

        async function deleteUser(serial) {
            if (confirm("确定删除该用户？")) {
                try {
                    await invoke("delete_user", { serial });
                    users.value = users.value.filter(u => u.device_id !== serial);
                } catch (error) {
                    console.error("删除用户失败:", error);
                }
            }
        }

        onMounted(fetchUsers);

        return {
            searchQuery, filteredUsers, updateUser, deleteUser, editingUserId,
            formatUserType,
            availableCategories,
            startEditing, selectCategory,
        };
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