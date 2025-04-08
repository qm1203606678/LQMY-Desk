<template>
    <div>
        <h1>LQMY 凌控 桌面端</h1>
        <div class="server-status">
            <h2>连接状态</h2>
            <p :class="statusClass">{{ statusMessage }}</p>
            <div class="buttons">
                <button :disabled="isRunning" @click="startServer">允许连接</button>
                <button :disabled="!isRunning" @click="stopServer">停止连接</button>
            </div>
        </div>

        <div class="server-info">
            <h2>服务器信息</h2>
            <p><strong>IP 地址:</strong> {{ serverAddress || "未获取" }}</p>
            <p><strong>连接口令:</strong> {{ connectionPassword || "无" }}</p>
            <button @click="fetchServerInfo">刷新服务器信息</button>
        </div>
    </div>
</template>

<script>
import { ref, computed, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";

export default {
    setup() {
        const isRunning = ref(false);
        const serverAddress = ref("");
        const connectionPassword = ref("");

        const statusMessage = computed(() => (isRunning.value ? "运行中" : "未启动"));
        const statusClass = computed(() => (isRunning.value ? "running" : "stopped"));

        async function fetchServerInfo() {
            try {
                const [address, password] = await invoke("get_server_info");
                serverAddress.value = address;
                connectionPassword.value = password;
            } catch (error) {
                console.error("获取服务器信息失败:", error);
            }
        }

        async function startServer() {
            try {
                await invoke("start_server");
                isRunning.value = true;
                fetchServerInfo();
            } catch (error) {
                console.error("启动服务器失败:", error);
            }
        }

        async function stopServer() {
            try {
                await invoke("stop_server");
                isRunning.value = false;
            } catch (error) {
                console.error("停止服务器失败:", error);
            }
        }

        onMounted(async () => {
            fetchServerInfo();
            try {
                const response = await fetch("http://127.0.0.1:9876/health");
                isRunning.value = response.ok;
            } catch (error) {
                isRunning.value = false;
            }
        });

        return { isRunning, serverAddress, connectionPassword, statusMessage, statusClass, startServer, stopServer, fetchServerInfo };
    }
};
</script>

<style scoped>
.server-status,
.server-info {
    background: #f8f8f8;
    padding: 20px;
    border-radius: 10px;
    display: inline-block;
    margin-top: 20px;
}

.running {
    color: green;
    font-weight: bold;
}

.stopped {
    color: red;
    font-weight: bold;
}

.buttons button {
    margin: 10px;
    padding: 10px 20px;
    font-size: 16px;
}
</style>