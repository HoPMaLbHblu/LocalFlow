import { forwardRef, useId, useState, type ReactNode } from "react";

interface Props {
  title: string;
  className?: string;
  id?: string;
  /** Controlled open state (optional); without it the card manages itself and starts closed. */
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  children: ReactNode;
}

/**
 * A settings card that folds away: only its title shows until it is opened.
 * The content stays mounted while closed, so nothing it loaded or typed is lost.
 */
const FoldCard = forwardRef<HTMLElement, Props>(function FoldCard({ title, className, id, open, onOpenChange, children }, ref) {
  const [ownOpen, setOwnOpen] = useState(false);
  const isOpen = open ?? ownOpen;
  const bodyId = useId();
  const toggle = () => {
    const next = !isOpen;
    if (open === undefined) setOwnOpen(next);
    onOpenChange?.(next);
  };
  return (
    <section className={`card fold-card ${isOpen ? "open" : ""} ${className ?? ""}`} id={id} ref={ref}>
      <button type="button" className="fold-head" aria-expanded={isOpen} aria-controls={bodyId} onClick={toggle}>
        <strong>{title}</strong>
        <span className="fold-chevron" aria-hidden="true">
          ▸
        </span>
      </button>
      <div className="fold-body" id={bodyId} hidden={!isOpen}>
        {children}
      </div>
    </section>
  );
});

export default FoldCard;
