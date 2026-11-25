import { ConnectionStatus, useConnectionStore } from '../../stores/connectionStore';

import style from './OfflineMarker.module.scss';

/**
 * A small mark in the corner of a view while the connection is lost. The view
 * keeps counting from the last state it was sent, so whoever reads it should
 * know the time may not include a change made meanwhile (plan 016).
 * The editor has its own, fuller notice (ConnectionIndicator).
 */
export default function OfflineMarker() {
  const isOffline = useConnectionStore((state) => state.status !== ConnectionStatus.Connected);
  if (!isOffline) return null;
  return (
    <div className={style.marker} role='status' aria-label='Offline' title='Offline — showing the last known time'>
      <span className={style.dot} />
    </div>
  );
}
