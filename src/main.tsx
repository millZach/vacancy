import './design/fonts';
import './design/tokens.css';
import './design/base.css';
import './design/controls.css';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import { BridgeProvider, defaultBridge } from './bridge';
import { DrivesProvider } from './drives/DrivesProvider';

const root = document.getElementById('root');
if (!root) throw new Error('index.html has no #root');

createRoot(root).render(
  <StrictMode>
    <BridgeProvider bridge={defaultBridge()}>
      <DrivesProvider>
        <App />
      </DrivesProvider>
    </BridgeProvider>
  </StrictMode>,
);
