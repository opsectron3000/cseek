import type { IconName } from './Icon';

export interface NavDef {
  to: string;
  label: string;
  icon: IconName;
  keywords?: string;
}

export const NAV_GROUPS: { label: string; items: NavDef[] }[] = [
  {
    label: 'Library',
    items: [
      { to: '/search', label: 'Search', icon: 'search', keywords: 'find query files' },
      { to: '/files', label: 'Files', icon: 'shares', keywords: 'shares library folders manager scan' },
      { to: '/downloads', label: 'Downloads', icon: 'download', keywords: 'transfers queue' },
      { to: '/uploads', label: 'Uploads', icon: 'upload', keywords: 'transfers queue' },
    ],
  },
  {
    label: 'Discover',
    items: [
      { to: '/wishlist', label: 'Wishlist', icon: 'wishlist', keywords: 'auto saved searches' },
      { to: '/discovery', label: 'Discovery', icon: 'discovery', keywords: 'recommendations interests similar' },
    ],
  },
  {
    label: 'People',
    items: [
      { to: '/chat', label: 'Chat', icon: 'message', keywords: 'messages rooms dm private public' },
      { to: '/users', label: 'Users', icon: 'user', keywords: 'buddies friends watch' },
    ],
  },
  {
    label: 'System',
    items: [{ to: '/system', label: 'System', icon: 'system', keywords: 'stats logs options settings' }],
  },
];
