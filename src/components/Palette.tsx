import { Search } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import { rankItems, type PaletteItem } from "../lib/palette";

/** ⌘K: find any chat, setting, mode, theme or action by typing a few letters. */
export function Palette({ items, onRun, onClose }: { items: PaletteItem[]; onRun: (item: PaletteItem) => void; onClose: () => void }) {
  const [q, setQ] = useState("");
  const [sel, setSel] = useState(0);
  const list = useMemo(() => rankItems(items, q, 14), [items, q]);
  const listRef = useRef<HTMLUListElement>(null);
  useEffect(() => setSel(0), [q]);
  useEffect(() => {
    listRef.current?.querySelector<HTMLElement>(`[data-i="${sel}"]`)?.scrollIntoView({ block: "nearest" });
  }, [sel]);

  const run = (it: PaletteItem | undefined) => {
    if (!it) return;
    onClose();
    onRun(it);
  };
  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setSel((s) => Math.min(s + 1, list.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSel((s) => Math.max(s - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      run(list[sel]);
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onClose();
    }
  };

  return (
    <div className="scrim palette-scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="palette" role="dialog" aria-modal="true" aria-label="Command palette">
        <div className="palette-input">
          <Search size={16} />
          <input
            autoFocus
            value={q}
            onChange={(e) => setQ(e.target.value)}
            onKeyDown={onKey}
            placeholder="Search chats, settings, modes, themes and actions"
            aria-label="Search commands"
            role="combobox"
            aria-expanded="true"
            aria-controls="palette-list"
            aria-activedescendant={list[sel] ? `pal-${list[sel].id}` : undefined}
          />
          <kbd>esc</kbd>
        </div>
        <ul className="palette-list" id="palette-list" role="listbox" ref={listRef}>
          {list.length === 0 && <li className="muted palette-empty">Nothing matches “{q}”.</li>}
          {list.map((it, i) => (
            <li key={it.id} id={`pal-${it.id}`} data-i={i} role="option" aria-selected={i === sel} className={i === sel ? "on" : ""} onMouseEnter={() => setSel(i)} onMouseDown={(e) => e.preventDefault()} onClick={() => run(it)}>
              <span className="palette-group">{it.group}</span>
              <span className="grow palette-label">{it.label}</span>
              {it.hint && <span className="palette-hint">{it.hint}</span>}
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
