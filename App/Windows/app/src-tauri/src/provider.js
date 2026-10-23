// Injected into every page of ihatecrypto's browser before its own scripts (the Mac's ProviderScript): ihatecrypto as a
// Solana wallet through the Wallet Standard and as an EIP-1193 provider announced through EIP-6963. Every call goes to
// the app, which puts it on the canvas for Confirm.
(() => {
  if (window.__ihatecryptoWallet) return;
  // Through the app's one command a site may call; the app reads the page's origin itself. Answers are JSON text.
  const call = async (method, params) => {
    const r = await window.__TAURI_INTERNALS__.invoke('dapp_call', { method, params });
    return method === 'eth' ? r : JSON.parse(r);
  };
  // WebView2 gives a page beside another webview no keyboard on a click: the app moves it here when it isn't.
  const toB64 = (bytes) => { let s = ''; for (const b of bytes) s += String.fromCharCode(b); return btoa(s); };
  const fromB64 = (text) => Uint8Array.from(atob(text), (c) => c.charCodeAt(0));
  const listeners = { change: [] };
  let accounts = [];
  const features = ['solana:signTransaction', 'solana:signAndSendTransaction', 'solana:signMessage'];
  const setAccount = (r) => {
    accounts = r && r.address ? [{ address: r.address, publicKey: fromB64(r.publicKey), chains: ['solana:mainnet'], features, label: 'ihatecrypto' }] : [];
    (listeners.change || []).forEach((f) => f({ accounts }));
  };
  const each = async (inputs, fn) => { const out = []; for (const input of inputs) out.push(await fn(input)); return out; };
  const wallet = {
    version: '1.0.0',
    name: 'ihatecrypto',
    icon: 'data:image/svg+xml;base64,' + btoa('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32"><circle cx="16" cy="16" r="15" fill="#FF7A2F"/><circle cx="16" cy="16" r="14.2" fill="none" stroke="#111" stroke-width="2" stroke-dasharray="1.6 1.6"/><circle cx="11.4" cy="14.2" r="3.9" fill="#fff" stroke="#111" stroke-width="1.2"/><circle cx="20.6" cy="14.2" r="3.9" fill="#fff" stroke="#111" stroke-width="1.2"/><circle cx="12.3" cy="11.9" r="1.8" fill="#111"/><circle cx="21.5" cy="11.9" r="1.8" fill="#111"/><path d="M12.3 22.4 Q16 21 19.7 21.9" stroke="#111" stroke-width="1.8" fill="none" stroke-linecap="round"/></svg>'),
    chains: ['solana:mainnet'],
    get accounts() { return accounts; },
    features: {
      'standard:connect': { version: '1.0.0', connect: async (input) => { setAccount(await call('connect', { silent: !!(input && input.silent) })); return { accounts }; } },
      'standard:disconnect': { version: '1.0.0', disconnect: async () => { await call('disconnect', {}); setAccount(null); } },
      'standard:events': { version: '1.0.0', on: (event, listener) => { (listeners[event] = listeners[event] || []).push(listener); return () => { listeners[event] = listeners[event].filter((l) => l !== listener); }; } },
      'solana:signTransaction': { version: '1.0.0', supportedTransactionVersions: ['legacy', 0],
        signTransaction: (...inputs) => each(inputs, async (i) => ({ signedTransaction: fromB64((await call('signTransaction', { transaction: toB64(i.transaction) })).signed) })) },
      'solana:signAndSendTransaction': { version: '1.0.0', supportedTransactionVersions: ['legacy', 0],
        signAndSendTransaction: (...inputs) => each(inputs, async (i) => ({ signature: fromB64((await call('signAndSendTransaction', { transaction: toB64(i.transaction) })).signature) })) },
      'solana:signMessage': { version: '1.0.0',
        signMessage: (...inputs) => each(inputs, async (i) => ({ signedMessage: i.message, signature: fromB64((await call('signMessage', { message: toB64(i.message) })).signature) })) },
    },
  };
  let registrations = 0;
  const register = ({ register }) => { registrations += 1; return register(wallet); };
  try { window.dispatchEvent(new CustomEvent('wallet-standard:register-wallet', { detail: register })); } catch (e) {}
  try { window.addEventListener('wallet-standard:app-ready', ({ detail }) => register(detail)); } catch (e) {}
  // The older path some wallet adapters still read, as Phantom's own registration keeps it too.
  try { (window.navigator.wallets = window.navigator.wallets || []).push(register); } catch (e) {}
  Object.defineProperty(window, '__ihatecryptoRegistrations', { get: () => registrations });

  // EVM: an EIP-1193 provider, announced through EIP-6963 so sites list «ihatecrypto» beside other wallets.
  // Errors come back as "code|message" and are rethrown with the EIP-1193 code (4001 refused, 4200 unsupported).
  const ethListeners = {};
  const ethEmit = (event, value) => (ethListeners[event] || []).forEach((f) => { try { f(value); } catch (e) {} });
  const ethCall = async (method, params) => {
    try {
      const r = await call('eth', { method, params: JSON.stringify(params === undefined ? [] : params) });
      return r === null || r === undefined ? null : JSON.parse(r);
    } catch (e) {
      const text = String((e && e.message) || e);
      const bar = text.indexOf('|');
      const error = new Error(bar > 0 ? text.slice(bar + 1) : text);
      error.code = bar > 0 ? Number(text.slice(0, bar)) : 4001;
      throw error;
    }
  };
  const ethereum = {
    isIhatecrypto: true,
    request: async ({ method, params }) => {
      const result = await ethCall(method, params);
      if (method === 'eth_requestAccounts') ethEmit('accountsChanged', result);
      if (method === 'wallet_switchEthereumChain') ethEmit('chainChanged', await ethCall('eth_chainId', []));
      return result;
    },
    on: (event, f) => { (ethListeners[event] = ethListeners[event] || []).push(f); return ethereum; },
    removeListener: (event, f) => { ethListeners[event] = (ethListeners[event] || []).filter((x) => x !== f); return ethereum; },
  };
  const info = Object.freeze({ uuid: (crypto.randomUUID && crypto.randomUUID()) || String(Math.random()), name: 'ihatecrypto', icon: wallet.icon, rdns: 'app.ihatecrypto' });
  const announce = () => window.dispatchEvent(new CustomEvent('eip6963:announceProvider', { detail: Object.freeze({ info, provider: ethereum }) }));
  window.addEventListener('eip6963:requestProvider', announce);
  announce();
  if (!window.ethereum) { try { Object.defineProperty(window, 'ethereum', { value: ethereum, configurable: true }); } catch (e) {} }
  Object.defineProperty(window, '__ihatecryptoWallet', { value: wallet });
})();

