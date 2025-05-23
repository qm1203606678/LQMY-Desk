// src/stores/server.js
import { defineStore } from 'pinia';
import { ref } from 'vue';

export const useServerStore = defineStore('server', () => {
  const isRunning = ref(false);
  const serverAddress = ref('');
  const connectionPassword = ref('');
  // const currentUser = ref({
  //   device_name: '',
  //   device_id: '!@#$%^&*()',
  //   user_type: 'Normal',
  // });
  const currentUuid = ref('');
  const curUsersInfo = ref({
    max: 0,
    pointer: 0,
    usersinfo: [],
  });

  return {
    isRunning,
    serverAddress,
    connectionPassword,
    //currentUser,
    currentUuid,
    curUsersInfo,
  };
});
