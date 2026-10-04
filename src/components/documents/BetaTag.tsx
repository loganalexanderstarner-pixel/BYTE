/** "Beta" mark for document making (PowerPoint slides especially still need work). */
export function BetaTag({
  title = "Still being improved: check the file before you share it.",
}: {
  title?: string;
}) {
  return (
    <span className="beta-tag" title={title}>
      Beta
    </span>
  );
}

export const BETA_NOTE =
  "Making documents is in beta. PDF and Word files are close to done; PowerPoint slides still need work, so look them over before you share them.";
