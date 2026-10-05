const {contextBridge, ipcRenderer} = require('electron');
contextBridge.exposeInMainWorld('fixture', {dialog:()=>ipcRenderer.invoke('fixture-dialog')});
