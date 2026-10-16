import { useEffect } from 'react';
import { Tooltip } from '@base-ui/react/tooltip';
import './lib/theme';
import { bootstrap, useStore } from './store/app';
import { FOCUS_SEARCH_EVENT, matchPath, navigate, routes, usePath } from './lib/router';
import { Layout } from './components/Layout';
import { ToastProvider } from './components/Toaster';
import { Spinner } from './components/ui';
import { LoginPage } from './pages/LoginPage';
import { SearchPage } from './pages/SearchPage';
import { TransfersPage } from './pages/TransfersPage';
import { ChatPage } from './pages/ChatPage';
import { UsersPage } from './pages/UsersPage';
import { WishlistPage } from './pages/WishlistPage';
import { DiscoveryPage } from './pages/DiscoveryPage';
import { FilesPage } from './pages/FilesPage';
import { SystemPage } from './pages/SystemPage';


function Routes() {
  const path = usePath();
  let m: Record<string, string> | null;
  if ((m = matchPath('/search/:id?', path))) return <SearchPage searchId={m.id} />;
  if (matchPath('/downloads', path)) return <TransfersPage direction="download" />;
  if (matchPath('/uploads', path)) return <TransfersPage direction="upload" />;
  if ((m = matchPath('/chat/u/:name', path))) return <ChatPage view={{ kind: 'user', name: m.name }} />;
  if ((m = matchPath('/chat/r/:name', path))) return <ChatPage view={{ kind: 'room', name: m.name }} />;
  if (matchPath('/chat/rooms', path)) return <ChatPage view={{ kind: 'rooms' }} />;
  if (matchPath('/chat', path)) return <ChatPage view={{ kind: 'home' }} />;
  // Old addresses.
  if ((m = matchPath('/messages/u/:name', path))) return <Redirect to={routes.dm(m.name)} />;
  if ((m = matchPath('/messages/r/:name', path))) return <Redirect to={routes.room(m.name)} />;
  if (matchPath('/messages', path)) return <Redirect to="/chat" />;
  if ((m = matchPath('/rooms/:name?', path))) return <Redirect to={m.name ? routes.room(m.name) : '/chat/rooms'} />;
  if ((m = matchPath('/users/:name?', path))) return <UsersPage username={m.name} />;
  if (matchPath('/wishlist', path)) return <WishlistPage />;
  if (matchPath('/discovery', path)) return <DiscoveryPage />;
  if ((m = matchPath('/files/*', path))) return <FilesPage rest={m.rest || undefined} />;
  if (matchPath('/files', path)) return <FilesPage />;
  if (matchPath('/shares/:tab?', path)) return <Redirect to="/files" />;
  if ((m = matchPath('/system/:tab?', path))) return <SystemPage tab={m.tab} />;
  return <Redirect to="/search" />;
}

function Redirect({ to }: { to: string }) {
  useEffect(() => navigate(to, { replace: true }), [to]);
  return null;
}

function useGlobalKeys() {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== '/' || e.metaKey || e.ctrlKey || e.altKey) return;
      const t = e.target as HTMLElement | null;
      if (t && (t.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName))) return;
      e.preventDefault();
      if (!location.pathname.startsWith('/search')) navigate('/search');
      // Let the page mount before focusing.
      requestAnimationFrame(() => window.dispatchEvent(new Event(FOCUS_SEARCH_EVENT)));
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);
}

export function App() {
  const status = useStore((s) => s.auth.status);
  useEffect(() => {
    void bootstrap();
  }, []);
  useGlobalKeys();

  return (
    <>
    <ToastProvider>
      <Tooltip.Provider delay={350} closeDelay={0}>
        {status === 'checking' ? (
          <div className="splash">
            <Spinner size={16} />
          </div>
        ) : status === 'login' ? (
          <LoginPage />
        ) : (
          <Layout>
            <Routes />
          </Layout>
        )}
      </Tooltip.Provider>
    </ToastProvider>
    </>
  );
}
