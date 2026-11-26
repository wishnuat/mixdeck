export function Speaker({ level, muted }: { level: number; muted: boolean }) {
  return (
    <svg viewBox="0 0 24 24" className="ico" aria-hidden>
      <path d="M4 9.5h3.2L12 5.5v13l-4.8-4H4z" fill="currentColor" stroke="none" />
      {muted ? (
        <path d="M16 9.5l5 5m0-5-5 5" />
      ) : (
        <>
          {level > 0.01 && <path d="M15.2 9.6a3.4 3.4 0 0 1 0 4.8" />}
          {level > 0.34 && <path d="M17.4 7.4a6.5 6.5 0 0 1 0 9.2" />}
          {level > 0.67 && <path d="M19.6 5.2a9.6 9.6 0 0 1 0 13.6" />}
        </>
      )}
    </svg>
  );
}

export const Gear = () => (
  <svg viewBox="0 0 24 24" className="ico" aria-hidden>
    <circle cx="12" cy="12" r="3" />
    <path d="M19.4 13.5a7.6 7.6 0 0 0 0-3l2-1.5-2-3.4-2.3.9a7.5 7.5 0 0 0-2.6-1.5L14 2.6h-4l-.5 2.4a7.5 7.5 0 0 0-2.6 1.5l-2.3-.9-2 3.4 2 1.5a7.6 7.6 0 0 0 0 3l-2 1.5 2 3.4 2.3-.9a7.5 7.5 0 0 0 2.6 1.5l.5 2.4h4l.5-2.4a7.5 7.5 0 0 0 2.6-1.5l2.3.9 2-3.4z" />
  </svg>
);

export const Back = () => (
  <svg viewBox="0 0 24 24" className="ico" aria-hidden><path d="M15 5l-7 7 7 7" /></svg>
);

export const Pin = () => (
  <svg viewBox="0 0 24 24" className="ico sm" aria-hidden>
    <path d="M9 3h6l-1 6 3 3v2H7v-2l3-3z" fill="currentColor" stroke="none" />
    <path d="M12 14v7" />
  </svg>
);

export const Chime = () => (
  <svg viewBox="0 0 24 24" className="ico" aria-hidden>
    <path d="M6 16V11a6 6 0 1 1 12 0v5l1.5 2h-15z" />
    <path d="M10 20a2 2 0 0 0 4 0" />
  </svg>
);

export const Chevron = ({ open }: { open: boolean }) => (
  <svg viewBox="0 0 24 24" className="ico sm chev" style={{ transform: open ? "rotate(90deg)" : "none" }} aria-hidden>
    <path d="M9 5l7 7-7 7" />
  </svg>
);

export const Bars = () => (
  <span className="bars" aria-hidden><i /><i /><i /></span>
);
