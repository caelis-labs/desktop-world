// App-owned event oracle. The test controller never executes renderer scripts.
const {app, BrowserWindow, Menu, ipcMain, dialog} = require('electron');
const fs = require('node:fs');
const path = require('node:path');
const config = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
app.setPath('userData', config.profile);
app.commandLine.appendSwitch('force-renderer-accessibility');
function record(event, value = '') {
  fs.appendFileSync(config.log, JSON.stringify({event, value, at: new Date().toISOString()}) + '\n');
}
app.whenReady().then(() => {
  app.setAccessibilitySupportEnabled(true);
  const win = new BrowserWindow({x: 260, y: 180, width: 780, height: 1000,
    webPreferences: {contextIsolation: true, nodeIntegration: false, preload: path.join(__dirname, 'preload.cjs')}});
  win.setMenu(Menu.buildFromTemplate([{label:'Fixture', submenu:[
    {label:'Record menu', accelerator:'CommandOrControl+Shift+M', click:()=>record('native_menu')},
    {label:'Exit fixture', click:()=>win.close()}
  ]}]));
  ipcMain.handle('fixture-dialog', async () => {
    record('native_dialog_open');
    const result = await dialog.showMessageBox(win, {title: config.dialogTitle,
      message: 'Confirm the fixture order', buttons:['Cancel', 'Confirm'], defaultId:0, cancelId:0});
    record('native_dialog_result', String(result.response));
  });
  win.webContents.on('dom-ready', () => record('ready', process.versions));
  win.loadURL(config.url);
});
app.on('window-all-closed', () => app.quit());
