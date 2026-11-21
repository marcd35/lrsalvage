import { capability } from './app.js';

const result = capability();
if (!result.ok) {
  const warning = document.getElementById('browser-warning');
  const messages = {
    'no-picker': 'This browser cannot open folders for recovery. Use Chrome or Edge on Windows, or use the CLI.',
    'no-isolation': 'Recovery needs cross-origin isolation. Open the hosted HTTPS page.',
    insecure: 'Recovery needs a secure browser context. Open the hosted HTTPS page.',
  };
  warning.textContent = messages[result.reason];
  warning.hidden = false;
}
