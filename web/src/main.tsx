import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import '@fontsource-variable/instrument-sans/standard.css';
import '@fontsource-variable/geist-mono';
import 'flag-icons/css/flag-icons.min.css';
import { App } from './App';
import './styles/global.css';

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
