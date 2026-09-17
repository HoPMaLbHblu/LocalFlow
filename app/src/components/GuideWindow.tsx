import { useEffect, useState } from "react";
import { t } from "../i18n";
import { sendDraftToMain } from "../windowing";
import GuidePage from "./GuidePage";

/** The guide on its own, in a separate window next to the app. */
export default function GuideWindow() {
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    document.title = t("guide.windowTitle");
  }, []);

  const openInApp = async (title: string, code: string) => {
    try {
      await sendDraftToMain(title, code);
      setNotice(t("guide.sentToEditor"));
      setTimeout(() => setNotice(null), 2500);
    } catch (e) {
      setNotice(String(e));
    }
  };

  return (
    <div className="guide-window">
      {notice && <div className="guide-toast">{notice}</div>}
      <GuidePage onTry={openInApp} />
    </div>
  );
}
