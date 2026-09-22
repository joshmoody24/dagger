import "./ReadingNav.css";

interface ReadingNavProps {
  at: number;
  total: number;
  read: boolean;
  onStep: (by: number) => void;
  onRead: (by: number) => void;
  onToggle: () => void;
}

export function ReadingNav(props: ReadingNavProps) {
  return (
    <div class="nav">
      <div class="pair">
        <button
          class="nav-quiet"
          disabled={props.at === 0}
          onClick={() => props.onStep(-1)}
          aria-label="Back, without marking"
        >
          <span>back</span>
          <kbd>p</kbd>
        </button>
        <button
          class="nav-quiet"
          disabled={props.at === props.total - 1}
          onClick={() => props.onStep(1)}
          aria-label="Next, without marking"
        >
          <span>next</span>
          <kbd>n</kbd>
        </button>
        <button
          class={`nav-progress${props.read ? " done" : ""}`}
          onClick={() => props.onToggle()}
          aria-label={props.read ? "Read. Press to unmark" : "Not read yet"}
        >
          <span>
            {props.read ? "✓ " : ""}
            {props.at + 1}/{props.total}
          </span>
          <kbd>m</kbd>
        </button>
        <button
          class="nav-primary"
          disabled={props.at === 0}
          onClick={() => props.onRead(-1)}
          aria-label="Read, and back"
        >
          <span>✓ back</span>
          <kbd>h</kbd>
        </button>
        <button
          class="nav-primary"
          onClick={() => props.onRead(1)}
          aria-label="Read, and next"
        >
          <span>✓ next</span>
          <kbd>l</kbd>
        </button>
      </div>
    </div>
  );
}
