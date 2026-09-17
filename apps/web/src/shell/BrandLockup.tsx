/**
 * The five-tine duckfoot lockup, from the turn-2 chrome.
 *
 * Turn 1 used three tines and a letterspaced sans wordmark; turn 2 splays five and
 * sets the name in Instrument Serif over a mono tagline. The tine heights
 * (7/11/15/11/7) and the dimmer outer pair are what read as splay rather than as a
 * bar chart.
 */
const TINES = [
  { height: 7, dim: true },
  { height: 11, dim: false },
  { height: 15, dim: false },
  { height: 11, dim: false },
  { height: 7, dim: true },
];

export function BrandLockup() {
  return (
    <div className="df-brand">
      <div className="df-brand__mark" aria-hidden="true">
        {TINES.map((tine, index) => (
          <span
            key={index}
            className={`df-brand__tine${tine.dim ? ' df-brand__tine--dim' : ''}`}
            style={{ height: `${tine.height}px` }}
          />
        ))}
      </div>
      <div className="df-brand__text">
        <span className="df-brand__word">Duckfoot</span>
        <span className="df-brand__tagline">THREE BARRELS, ONE STOCK</span>
      </div>
    </div>
  );
}
