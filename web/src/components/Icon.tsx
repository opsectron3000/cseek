import type { SVGProps } from 'react';

const PATHS = {
  search: 'M7 12.5a5.5 5.5 0 1 0 0-11 5.5 5.5 0 0 0 0 11ZM11 11l3.5 3.5',
  download: 'M8 2v8.5M4.5 7 8 10.5 11.5 7M2.5 13.5h11',
  upload: 'M8 13V4.5M4.5 8 8 4.5 11.5 8M2.5 2.5h11',
  message: 'M2.5 3.5h11v7.5h-6.5L4 13.5V11H2.5z',
  rooms: 'M2.5 3.5h7v5h-4L3.5 10V8.5h-1zM9.5 6h4v5h-1v1.5L10.5 11H7V8.5',
  user: 'M8 7.5a2.75 2.75 0 1 0 0-5.5 2.75 2.75 0 0 0 0 5.5ZM2.5 14c.5-3 2.8-4.5 5.5-4.5s5 1.5 5.5 4.5',
  wishlist: 'M3.5 2.5h9v11L8 10.5l-4.5 3z',
  discovery: 'M8 14.5a6.5 6.5 0 1 0 0-13 6.5 6.5 0 0 0 0 13ZM10.5 5.5 9 9l-3.5 1.5L7 7z',
  shares: 'M1.5 4V13h13V5.5H7.5L6 4z',
  system: 'M2.5 4.5h11M2.5 8h11M2.5 11.5h11M5 3v3M11 6.5v3M7 10v3',
  play: 'M5 3.2v9.6L12.5 8z',
  pause: 'M5 3v10M11 3v10',
  next: 'M4 3.5v9L10 8zM12 3.5v9',
  prev: 'M12 3.5v9L6 8zM4 3.5v9',
  volume: 'M2.5 6v4h2.5l3.5 3V3L5 6zM11 5.5a3.5 3.5 0 0 1 0 5',
  mute: 'M2.5 6v4h2.5l3.5 3V3L5 6zM11 6l3 4M14 6l-3 4',
  x: 'M4 4l8 8M12 4l-8 8',
  check: 'M3 8.5 6.5 12 13 4.5',
  refresh: 'M13 8a5 5 0 1 1-1.5-3.6M13 2.5V5h-2.5',
  more: 'M3.5 8h.01M8 8h.01M12.5 8h.01',
  chevronRight: 'M6 3.5 10.5 8 6 12.5',
  chevronLeft: 'M10 3.5 5.5 8l4.5 4.5',
  chevronDown: 'M3.5 6 8 10.5 12.5 6',
  folder: 'M1.5 4V13h13V5.5H7.5L6 4z',
  file: 'M4 1.5h5.5l3 3v10H4zM9.5 1.5v3h3',
  music: 'M6 12V3.5l7-1.5V10.5M6 12a2 2 0 1 1-4 0 2 2 0 0 1 4 0ZM13 10.5a2 2 0 1 1-4 0 2 2 0 0 1 4 0Z',
  trash: 'M2.5 4h11M6 4V2.5h4V4M4 4l.7 9.5h6.6L12 4',
  plus: 'M8 3v10M3 8h10',
  queue: 'M2.5 4h8M2.5 8h8M2.5 12h5M12.5 9.5v5M10 12h5',
  stop: 'M4 4h8v8H4z',
  lock: 'M4 7.5h8v6H4zM5.5 7.5V5a2.5 2.5 0 0 1 5 0v2.5',
  menu: 'M2.5 4h11M2.5 8h11M2.5 12h11',
  logout: 'M6.5 2.5h-4v11h4M10.5 5l3 3-3 3M13.5 8H6',
  external: 'M9 2.5h4.5V7M13.5 2.5 7.5 8.5M11.5 9.5v4h-9v-9h4',
  command: 'M5.5 10.5h5v-5h-5zM5.5 5.5V4A1.5 1.5 0 1 0 4 5.5h1.5M10.5 5.5V4A1.5 1.5 0 1 1 12 5.5h-1.5M5.5 10.5V12A1.5 1.5 0 1 1 4 10.5h1.5M10.5 10.5V12a1.5 1.5 0 1 0 1.5-1.5h-1.5',
  sun: 'M8 10.5a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5ZM8 1.5v1.5M8 13v1.5M1.5 8H3M13 8h1.5M3.4 3.4l1 1M11.6 11.6l1 1M3.4 12.6l1-1M11.6 4.4l1-1',
  moon: 'M13.5 9.5A5.5 5.5 0 0 1 6.5 2.5a5.5 5.5 0 1 0 7 7Z',
  monitor: 'M2 3h12v8H2zM5.5 14h5M8 11v3',
  arrowRight: 'M3 8h10M9 4l4 4-4 4',
  arrowUp: 'M8 13V3M4 7l4-4 4 4',
  arrowDown: 'M8 3v10M4 9l4 4 4-4',
  enter: 'M13 3.5V8a1.5 1.5 0 0 1-1.5 1.5H3M6 6.5l-3 3 3 3',
  globe: 'M8 14.5a6.5 6.5 0 1 0 0-13 6.5 6.5 0 0 0 0 13ZM1.5 8h13M8 1.5c1.8 1.8 2.6 4 2.6 6.5S9.8 12.7 8 14.5C6.2 12.7 5.4 10.5 5.4 8S6.2 3.3 8 1.5Z',
  bolt: 'M9 1.5 3.5 9H8l-1 5.5L12.5 7H8z',
  power: 'M8 2v6M4.6 4.2a5 5 0 1 0 6.8 0',
  clock: 'M8 14.5a6.5 6.5 0 1 0 0-13 6.5 6.5 0 0 0 0 13ZM8 4.5V8l2.5 1.5',
  filter: 'M2 3.5h12L9.5 9v4l-3 1.5V9z',
  copy: 'M5.5 5.5h8v8h-8zM10.5 5.5v-3h-8v8h3',
  heart: 'M8 13.5S2 10 2 6a3 3 0 0 1 6-1 3 3 0 0 1 6 1c0 4-6 7.5-6 7.5Z',
  star: 'M8 1.8l1.9 3.9 4.3.6-3.1 3 .7 4.3L8 11.6l-3.8 2 .7-4.3-3.1-3 4.3-.6z',
  sliders: 'M2.5 4.5h6M11.5 4.5h2M2.5 11.5h2M7.5 11.5h6M10 3v3M6 10v3',
  eye: 'M1.5 8S4 3.5 8 3.5 14.5 8 14.5 8 12 12.5 8 12.5 1.5 8 1.5 8ZM8 10a2 2 0 1 0 0-4 2 2 0 0 0 0 4Z',
  ban: 'M8 14.5a6.5 6.5 0 1 0 0-13 6.5 6.5 0 0 0 0 13ZM3.4 3.4l9.2 9.2',
  info: 'M8 14.5a6.5 6.5 0 1 0 0-13 6.5 6.5 0 0 0 0 13ZM8 7.5v4M8 5h.01',
  alert: 'M8 2 1.5 13.5h13zM8 6.5v3M8 11.5h.01',
  hash: 'M3 6h10.5M2.5 10H13M6.5 2.5l-1 11M10.5 2.5l-1 11',
  send: 'M14 2 7 9M14 2l-4.5 12L7 9 2 6.5z',
  link: 'M7 9a3 3 0 0 0 4.2.3l2-2a3 3 0 0 0-4.2-4.2l-1 1M9 7a3 3 0 0 0-4.2-.3l-2 2a3 3 0 0 0 4.2 4.2l1-1',
  pin: 'M10 1.5 14.5 6 12 7l-2.5 2.5L10 13 3 6l3.5.5L9 4z M5.5 10.5 1.5 14.5',
} as const;

export type IconName = keyof typeof PATHS;

export function Icon({ name, size = 16, ...rest }: { name: IconName; size?: number } & SVGProps<SVGSVGElement>) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.4}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
      className="icon"
      {...rest}
    >
      <path d={PATHS[name]} />
    </svg>
  );
}
