import '@xterm/xterm/css/xterm.css';
import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { FitAddon } from '@xterm/addon-fit';
import { WebglAddon } from '@xterm/addon-webgl';
import { Terminal } from '@xterm/xterm';

import {
  PROTOCOL_VERSION,
  encodeBinaryInput,
  encodeControl,
  encodeTerminalInput,
  parseServerMessage,
  type ClientMessage,
} from './protocol';
import './styles.css';

const terminalElement = document.querySelector<HTMLElement>('#terminal');
const statusElement = document.querySelector<HTMLElement>('#connection-status');
const restartElement = document.querySelector<HTMLButtonElement>('#restart');
const windowControlsElement = document.querySelector<HTMLElement>('#window-controls');
const minimizeElement = document.querySelector<HTMLButtonElement>('#window-minimize');
const maximizeElement = document.querySelector<HTMLButtonElement>('#window-maximize');
const closeElement = document.querySelector<HTMLButtonElement>('#window-close');

if (
  !terminalElement ||
  !statusElement ||
  !restartElement ||
  !windowControlsElement ||
  !minimizeElement ||
  !maximizeElement ||
  !closeElement
) {
  throw new Error('R-SSH Web terminal markup is incomplete');
}

const status = statusElement;
const restart = restartElement;
const windowControls = windowControlsElement;
const minimize = minimizeElement;
const maximize = maximizeElement;
const close = closeElement;
const SOCKET_INPUT_HIGH_WATER_BYTES = 1_048_576;

function setMaximizedState(value: boolean): void {
  maximize.dataset.maximized = String(value);
  maximize.setAttribute('aria-label', value ? 'Restore window' : 'Maximize window');
  maximize.title = value ? 'Restore window' : 'Maximize window';
  const glyph = maximize.querySelector('span');
  if (glyph) {
    glyph.textContent = value ? '❐' : '□';
  }
}

function isMacOSWindow(): boolean {
  return /Macintosh|Mac OS X/.test(navigator.userAgent);
}

async function setupTauriWindowControls(): Promise<void> {
  if (!isTauri()) {
    return;
  }

  const appWindow = getCurrentWindow();
  const macOS = isMacOSWindow();
  document.documentElement.dataset.tauriWindow = 'true';
  windowControls.dataset.platform = macOS ? 'macos' : 'other';
  windowControls.hidden = false;
  minimize.addEventListener('click', () => void appWindow.minimize());
  maximize.addEventListener('click', () => {
    void appWindow
      .toggleMaximize()
      .then(() => appWindow.isMaximized())
      .then(setMaximizedState)
      .catch(() => {});
  });
  close.addEventListener('click', () => void appWindow.close());

  try {
    setMaximizedState(await appWindow.isMaximized());
    await appWindow.onResized(() => {
      void appWindow.isMaximized().then(setMaximizedState);
    });
  } catch {
    // Window state is optional; the controls remain usable without it.
  }
}

void setupTauriWindowControls();

const terminal = new Terminal({
  allowTransparency: false,
  cursorBlink: true,
  cursorStyle: 'block',
  fontFamily: '"SFMono-Regular", "Cascadia Code", "Roboto Mono", Menlo, Consolas, monospace',
  fontSize: 14,
  scrollback: 10_000,
  theme: {
    background: '#0d1117',
    foreground: '#d6deeb',
    cursor: '#79c0ff',
    selectionBackground: '#264f78',
    black: '#0d1117',
    red: '#ff7b72',
    green: '#7ee787',
    yellow: '#d2a8ff',
    blue: '#79c0ff',
    magenta: '#d2a8ff',
    cyan: '#a5d6ff',
    white: '#f0f6fc',
    brightBlack: '#484f58',
    brightRed: '#ffa198',
    brightGreen: '#56d364',
    brightYellow: '#e3b341',
    brightBlue: '#a5d6ff',
    brightMagenta: '#d2a8ff',
    brightCyan: '#b3f0ff',
    brightWhite: '#ffffff',
  },
});
const fitAddon = new FitAddon();
terminal.loadAddon(fitAddon);
terminal.open(terminalElement);

try {
  const webglAddon = new WebglAddon();
  webglAddon.onContextLoss(() => webglAddon.dispose());
  terminal.loadAddon(webglAddon);
} catch {
  // The default renderer remains active when WebGL2 is unavailable.
}

let socket: WebSocket | null = null;
let sessionOpen = false;
let closeRequested = false;
let resizeFrame: number | null = null;
let lastSize = { cols: 0, rows: 0 };

function setStatus(state: 'connecting' | 'open' | 'closing' | 'exited' | 'failed', text: string): void {
  status.dataset.state = state;
  status.textContent = text;
  restart.hidden = state !== 'exited' && state !== 'failed';
}

function sendControl(message: ClientMessage): void {
  if (socket?.readyState === WebSocket.OPEN) {
    socket.send(encodeControl(message));
  }
}

function sendTerminalInput(bytes: Uint8Array): void {
  if (!sessionOpen || !socket || socket.readyState !== WebSocket.OPEN) {
    return;
  }
  if (socket.bufferedAmount > SOCKET_INPUT_HIGH_WATER_BYTES) {
    sessionOpen = false;
    setStatus('failed', 'Connection backpressure');
    socket.close(1009, 'input backpressure');
    return;
  }
  socket.send(bytes);
}

function fitAndResize(): void {
  resizeFrame = null;
  fitAddon.fit();
  const nextSize = { cols: terminal.cols, rows: terminal.rows };
  if (nextSize.cols === lastSize.cols && nextSize.rows === lastSize.rows) {
    return;
  }
  lastSize = nextSize;
  if (sessionOpen) {
    sendControl({ type: 'resize', ...nextSize });
  }
}

function scheduleFit(): void {
  if (resizeFrame === null) {
    resizeFrame = requestAnimationFrame(fitAndResize);
  }
}

function closeSession(): void {
  if (!socket || socket.readyState !== WebSocket.OPEN || closeRequested) {
    return;
  }
  closeRequested = true;
  sessionOpen = false;
  setStatus('closing', 'Closing');
  sendControl({ type: 'close' });
}

function connect(): void {
  if (socket && socket.readyState !== WebSocket.CLOSED) {
    socket.close();
  }
  setStatus('connecting', 'Connecting');
  terminal.clear();
  terminal.reset();
  closeRequested = false;
  sessionOpen = false;
  fitAddon.fit();
  lastSize = { cols: terminal.cols, rows: terminal.rows };

  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  const nextSocket = new WebSocket(`${protocol}//${window.location.host}/api/v1/terminal`);
  socket = nextSocket;
  nextSocket.binaryType = 'arraybuffer';

  nextSocket.addEventListener('open', () => {
    if (socket !== nextSocket) {
      return;
    }
    sendControl({
      type: 'open',
      protocol: PROTOCOL_VERSION,
      cols: terminal.cols,
      rows: terminal.rows,
      profile: 'local-default',
    });
  });

  nextSocket.addEventListener('message', (event: MessageEvent<string | ArrayBuffer>) => {
    if (socket !== nextSocket) {
      return;
    }
    if (typeof event.data !== 'string') {
      terminal.write(new Uint8Array(event.data));
      return;
    }
    const message = parseServerMessage(event.data);
    if (!message) {
      setStatus('failed', 'Invalid server message');
      return;
    }
    if (message.type === 'opened') {
      sessionOpen = true;
      lastSize = { cols: message.cols, rows: message.rows };
      setStatus('open', 'Connected');
      terminal.focus();
      return;
    }
    if (message.type === 'exit') {
      sessionOpen = false;
      setStatus('exited', `Exited (${message.code})`);
      terminal.writeln(`\r\n[process exited with code ${message.code}]`);
      return;
    }
    if (message.fatal) {
      sessionOpen = false;
      setStatus('failed', message.message);
      terminal.writeln(`\r\n[${message.code}] ${message.message}`);
    }
  });

  nextSocket.addEventListener('close', () => {
    if (socket !== nextSocket) {
      return;
    }
    sessionOpen = false;
    if (closeRequested) {
      setStatus('exited', 'Closed');
    } else if (status.dataset.state !== 'exited') {
      setStatus('failed', 'Disconnected');
    }
  });
  nextSocket.addEventListener('error', () => {
    if (socket !== nextSocket) {
      return;
    }
    if (!closeRequested) {
      setStatus('failed', 'Connection failed');
    }
  });
}

terminal.onData((data) => {
  sendTerminalInput(encodeTerminalInput(data));
});

terminal.onBinary((data) => {
  sendTerminalInput(encodeBinaryInput(data));
});

terminal.onResize(({ cols, rows }) => {
  if (sessionOpen && (cols !== lastSize.cols || rows !== lastSize.rows)) {
    lastSize = { cols, rows };
    sendControl({ type: 'resize', cols, rows });
  }
});

const resizeObserver = new ResizeObserver(scheduleFit);
resizeObserver.observe(terminalElement);
window.addEventListener('beforeunload', closeSession);

connect();
restart.addEventListener('click', connect);
