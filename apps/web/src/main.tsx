import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { tokensToCssText } from '@duckfoot/core';

// Self-hosted, so first paint never touches the network. The design board links
// Google Fonts; porting that would contradict the README's zero-cloud claim and
// would be blocked outright by the desktop build's content-security policy.
import '@fontsource/ibm-plex-sans/400.css';
import '@fontsource/ibm-plex-sans/500.css';
import '@fontsource/ibm-plex-sans/600.css';
import '@fontsource/ibm-plex-mono/400.css';
import '@fontsource/ibm-plex-mono/500.css';
import '@fontsource/instrument-serif/400.css';

import '@duckfoot/ui/ui.css';
import './styles/shell.css';

import { App } from './App';

// Tokens are injected rather than written as a stylesheet so that TypeScript stays
// the single source of truth — canvas and SVG read the same values at draw time.
const style = document.createElement('style');
style.id = 'df-tokens';
style.textContent = tokensToCssText();
document.head.prepend(style);

const root = document.getElementById('root');
if (!root) throw new Error('#root missing from index.html');

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>
);
