import { useEffect, useState } from "react";
import { api, onUpdateAvailable, type UpdateInfo } from "../api";
import { t } from "../i18n";

/** "LocalFlow 1.4.0 is available" at the top of the main window, until updated or dismissed. */
export default function UpdateBanner() {
  const [update, setUpdate] = useState<UpdateInfo | null>(null);

  useEffect(() => {
    api.updateStatus(false).then(setUpdate).catch(() => {});
    const unlisten = onUpdateAvailable(setUpdate);
    return () => {
      unlisten.then((stop) => stop()).catch(() => {});
    };
  }, []);

  if (!update) return null;
  const { latest, current } = update;

  return (
    <div className="banner info update-banner">
      <span>{t("update.available", { version: latest.version, current })}</span>
      <button className="primary small" onClick={() => api.openReleasePage(latest.url).catch(() => {})}>
        {t("update.download")}
      </button>
      <button
        className="link small"
        onClick={() => {
          api.dismissUpdate(latest.version).catch(() => {});
          setUpdate(null);
        }}
      >
        {t("update.later")}
      </button>
    </div>
  );
}
