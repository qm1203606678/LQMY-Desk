<template>
    <div>
        <h1>LQMY 凌控 桌面端</h1>
        <div class="server-status">
            <h2>连接状态</h2>
            <p :class="statusClass">{{ statusMessage }}</p>
            <div>
                <label for="ip-input"><strong>IP 地址:</strong></label>
                <input id="ip-input" v-model="inputIp" placeholder="请输入服务器 IP 地址" />
                <button @click="confirmIp">确认</button>
            </div>
            <div class="buttons">
                <button :disabled="isRunning" @click="startServer">开启服务</button>
                <button :disabled="!isRunning" @click="stopServer">关闭服务</button>
            </div>
        </div>

        <div class="server-info">
            <h2>连接信息</h2>

            <p><strong>当前服务器IP 地址:</strong> {{ serverAddress || "未获取" }}</p>
            <p><strong>连接口令:</strong> {{ connectionPassword || "无" }}</p>
            <p><strong>本机编号:</strong>{{ currentUuid }}</p>
            <div class="user-info-card">
                <h3>当前用户</h3>
                <template v-if="currentUser.device_id !== '!@#$%^&*()'">
                    <p><strong>设备名称:</strong> {{ currentUser.device_name }}</p>
                    <p><strong>设备ID:</strong> {{ currentUser.device_id }}</p>
                    <p><strong>用户类型:</strong> {{ currentUser.user_type }}</p>

                </template>
                <p v-else>当前无设备连接</p>
            </div>
            <button @click="fetchServerInfo">刷新服务器信息</button>
        </div>
    </div>
</template>

<script>
import { ref, computed, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { useServerStore } from "../stores/server";

export default {
    setup() {
        const serverStore = useServerStore();
        const inputIp = ref("");
        const statusMessage = computed(() => (serverStore.isRunning ? "运行中" : "未启动"));
        const statusClass = computed(() => (serverStore.isRunning ? "running" : "stopped"));

        let timerId = null;

        serverStore.updateServerInfo = function (addr, pw, name, id, type, uuid, isRunning) {
            serverStore.serverAddress = addr;
            serverStore.connectionPassword = pw;
            serverStore.currentUser.device_name = name;
            serverStore.currentUser.device_id = id;
            serverStore.currentUser.user_type = type;
            serverStore.currentUuid = uuid
            serverStore.isRunning = isRunning
        }

        async function fetchServerInfo() {
            try {
                const [address, password, name, id, type, uuid, isRunning] = await invoke("get_server_info");
                serverStore.updateServerInfo(address, password, name, id, type, uuid, isRunning);
            } catch (error) {
                console.error("获取服务器信息失败:", error);
            }
        }

        async function startServer() {
            try {
                await invoke("start_server");
                serverStore.isRunning = true;
                fetchServerInfo();
            } catch (error) {
                console.error("启动服务器失败:", error);
            }
        }

        async function stopServer() {
            try {
                await invoke("stop_server");
                serverStore.isRunning = false;
                fetchServerInfo();
            } catch (error) {
                console.error("停止服务器失败:", error);
            }
        }

        async function confirmIp() {
            try {
                if (serverStore.isRunning) {
                    throw "请先断开当前服务"
                }
                await invoke("update_server_addr", { ipaddr: inputIp.value });
                //await saveIpToCache(inputIp.value); // 缓存 IP
                console.log("服务器地址已更新为:", inputIp.value);
            } catch (error) {
                console.error("更新服务器地址失败:", error);
                alert("更新服务器地址失败:", error);
            }
        }
        onMounted(async () => {
            fetchServerInfo();
            timerId = setInterval(fetchServerInfo, 5000);
            // try {
            //     const response = await fetch("http://127.0.0.1:9876/health");
            //     serverStore.isRunning = response.ok;
            // } catch (error) {
            //     serverStore.isRunning = false;
            // }
        });

        onUnmounted(() => {
            clearInterval(timerId);
        });

        return {
            inputIp,
            statusMessage,
            statusClass,
            startServer,
            stopServer,
            fetchServerInfo,
            confirmIp,
            serverAddress: computed(() => serverStore.serverAddress),
            connectionPassword: computed(() => serverStore.connectionPassword),
            currentUser: computed(() => serverStore.currentUser),
            isRunning: computed(() => serverStore.isRunning),
            currentUuid: computed(() => serverStore.currentUuid)
        };
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

.user-info-card {
    background: white;
    padding: 15px;
    border-radius: 8px;
    box-shadow: 0 2px 4px rgba(0, 0, 0, 0.1);
    margin: 15px 0;
}
</style>