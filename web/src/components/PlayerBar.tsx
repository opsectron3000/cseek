import { Slider } from '@base-ui/react/slider';
import { Popover } from '@base-ui/react/popover';
import { next, playAt, prev, removeAt, seek, setVolume, stop, toggle, usePlayer } from '../store/player';
import { formatDuration } from '../lib/format';
import { Icon } from './Icon';
import { Count, Empty, IconButton } from './ui';
import './player.css';

function QueuePopover() {
  const queue = usePlayer((s) => s.queue);
  const index = usePlayer((s) => s.index);
  return (
    <Popover.Root>
      <Popover.Trigger className="btn btn-ghost btn-icon player-queue-trigger" aria-label={`Queue (${queue.length})`} title="Queue">
        <Icon name="queue" size={15} />
        {queue.length > 1 && <Count n={queue.length} />}
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Positioner className="popover-positioner" side="top" align="end" sideOffset={8}>
          <Popover.Popup className="popover queue-popover">
            <Popover.Title className="popover-title">
              Up next <span className="faint num">{queue.length}</span>
            </Popover.Title>
            {queue.length === 0 ? (
              <Empty icon="queue">Queue is empty.</Empty>
            ) : (
              <ol className="queue-list">
                {queue.map((t, i) => (
                  <li key={`${t.key}${i}`} className={i === index ? 'current' : undefined}>
                    <span className="queue-n">{i === index ? <Bars /> : i + 1}</span>
                    <button type="button" className="queue-item" onClick={() => playAt(i)}>
                      <span className="truncate">{t.title}</span>
                      {t.artist && <span className="muted truncate small">{t.artist}</span>}
                    </button>
                    <IconButton icon="x" size="sm" label="Remove from queue" className="queue-x" onClick={() => removeAt(i)} />
                  </li>
                ))}
              </ol>
            )}
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  );
}

/** Tiny animated level meter; freezes when paused. */
function Bars({ paused }: { paused?: boolean }) {
  return (
    <span className={`bars${paused ? ' paused' : ''}`} aria-hidden="true">
      <span />
      <span />
      <span />
    </span>
  );
}

export function PlayerBar() {
  const track = usePlayer((s) => s.queue[s.index]);
  const playing = usePlayer((s) => s.playing);
  const currentTime = usePlayer((s) => s.currentTime);
  const duration = usePlayer((s) => s.duration);
  const volume = usePlayer((s) => s.volume);
  const error = usePlayer((s) => s.error);
  const hasNext = usePlayer((s) => s.index + 1 < s.queue.length);
  const hasPrev = usePlayer((s) => s.index > 0);
  if (!track) return null;

  return (
    <section className="player" aria-label="Player">
      <div className="player-controls">
        <IconButton icon="prev" label="Previous" onClick={prev} disabled={!hasPrev && currentTime < 1} />
        <IconButton icon={playing ? 'pause' : 'play'} label={playing ? 'Pause' : 'Play'} variant="primary" filled={!playing} onClick={toggle} className="player-play" />
        <IconButton icon="next" label="Next" onClick={next} disabled={!hasNext} />
      </div>
      <div className="player-main">
          <div
            key={track.key}
            className="player-title"
          >
            <span className="player-art">{error ? <Icon name="alert" size={13} /> : <Bars paused={!playing} />}</span>
            <span className="truncate medium">{track.title}</span>
            {(track.artist || track.album) && <span className="truncate faint">{[track.artist, track.album].filter(Boolean).join(' — ')}</span>}
            {error && <span className="player-error truncate">{error}</span>}
          </div>
        <div className="player-seek">
          <span className="mono">{formatDuration(currentTime)}</span>
          <Slider.Root
            className="slider"
            value={Math.min(currentTime, duration || 0)}
            min={0}
            max={duration || 1}
            step={1}
            disabled={!duration}
            onValueChange={(v) => seek(v)}
            aria-label="Seek"
          >
            <Slider.Control className="slider-control">
              <Slider.Track className="slider-track">
                <Slider.Indicator className="slider-indicator" />
                <Slider.Thumb className="slider-thumb" aria-label="Seek" />
              </Slider.Track>
            </Slider.Control>
          </Slider.Root>
          <span className="mono">{formatDuration(duration)}</span>
        </div>
      </div>
      <div className="player-side">
        <IconButton icon={volume === 0 ? 'mute' : 'volume'} label={volume === 0 ? 'Unmute' : 'Mute'} onClick={() => setVolume(volume === 0 ? 0.8 : 0)} />
        <Slider.Root className="slider slider-volume" value={volume} min={0} max={1} step={0.01} onValueChange={(v) => setVolume(v)}>
          <Slider.Control className="slider-control">
            <Slider.Track className="slider-track">
              <Slider.Indicator className="slider-indicator" />
              <Slider.Thumb className="slider-thumb" aria-label="Volume" />
            </Slider.Track>
          </Slider.Control>
        </Slider.Root>
        <QueuePopover />
        <IconButton icon="x" label="Close player" onClick={stop} />
      </div>
    </section>
  );
}
