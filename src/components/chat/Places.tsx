import { openUrl } from "@tauri-apps/plugin-opener";
import { Clock, Globe, MapPin, Navigation } from "lucide-react";

import type { PlacesFound, Spot } from "../../lib/types";

export function distanceText(m: number, imperial: boolean): string {
  if (imperial) {
    const mi = m / 1609.34;
    return mi < 0.1 ? `${Math.round((m * 3.281) / 10) * 10} ft` : `${mi.toFixed(1)} mi`;
  }
  return m < 1000 ? `${Math.round(m / 10) * 10} m` : `${(m / 1000).toFixed(1)} km`;
}

/** Apple Maps link (opens the Maps app on a Mac; the web map elsewhere). */
export function mapsUrl(s: Spot): string {
  return `https://maps.apple.com/?q=${encodeURIComponent(s.name)}&ll=${s.lat.toFixed(6)},${s.lon.toFixed(6)}`;
}

/** Cards for places found nearby, closest first. */
export function PlacesCards({ found }: { found: PlacesFound }) {
  if (found.spots.length === 0) return null;
  const open = (url: string) => void openUrl(url);
  return (
    <div className="places" role="region" aria-label={`${found.what} near ${found.near}`}>
      <div className="places-head">
        <MapPin size={14} /> {found.what[0].toUpperCase() + found.what.slice(1)} near {found.near}
        <span className="hint">From OpenStreetMap · hours may be out of date</span>
      </div>
      <div className="places-grid">
        {found.spots.slice(0, 8).map((s) => (
          <div key={s.osmUrl} className="place-card">
            <div className="place-top">
              <span className="place-name">{s.name}</span>
              <span className="place-dist">{distanceText(s.distanceM, found.imperial)}</span>
            </div>
            <div className="place-meta">
              {[s.kind, s.cuisine].filter(Boolean).join(" · ")}
              {s.openNow != null && <span className={`open-badge ${s.openNow ? "open" : "closed"}`}>{s.openNow ? "Open now" : "Closed"}</span>}
            </div>
            {s.address && <div className="place-addr">{s.address}</div>}
            {s.hours && (
              <div className="place-hours" title={s.hours}>
                <Clock size={12} />
                <span>{s.hours}</span>
              </div>
            )}
            <div className="place-actions">
              <button className="btn sm ghost" onClick={() => open(mapsUrl(s))}>
                <Navigation size={13} /> Maps
              </button>
              {s.website && (
                <button className="btn sm ghost" onClick={() => open(s.website)}>
                  <Globe size={13} /> Website
                </button>
              )}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
