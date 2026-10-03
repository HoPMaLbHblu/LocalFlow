import { useEffect, useRef, useState } from "react";
import QRCode from "qrcode";
import { api, errorMessages, onPhoneEvent, PHONE_PERMISSIONS, type PhonePermission, type PhoneStatus } from "../api";
import { t } from "../i18n";
import type { Key } from "../i18n/en";
import FoldCard from "./FoldCard";

const PAIR_SECONDS = 300;

/** Settings › Phone remote: pair the LocalFlow Remote app and choose what each phone may do. */
export default function PhoneCard() {
  const [status, setStatus] = useState<PhoneStatus | null>(null);
  const [open, setOpen] = useState(false);
  const [qr, setQr] = useState<string | null>(null);
  const [left, setLeft] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const cardRef = useRef<HTMLElement>(null);

  const load = () => api.phoneStatus().then(setStatus).catch(() => {});

  useEffect(() => {
    load();
    const off = onPhoneEvent(load);
    return () => {
      off.then((f) => f());
    };
  }, []);

  // A phone is waiting for "Allow?": open the card and show it.
  const pending = status?.pending ?? [];
  useEffect(() => {
    if (pending.length > 0) {
      setOpen(true);
      setQr(null);
      cardRef.current?.scrollIntoView({ behavior: "smooth", block: "center" });
    }
  }, [pending.length]);

  // The pairing code is valid for 5 minutes.
  useEffect(() => {
    if (!qr) return;
    const timer = setInterval(() => setLeft((s) => (s <= 1 ? (setQr(null), 0) : s - 1)), 1000);
    return () => clearInterval(timer);
  }, [qr]);

  const act = async (action: () => Promise<unknown>) => {
    setBusy(true);
    setError("");
    try {
      await action();
      await load();
    } catch (e) {
      setError(errorMessages(e).join(" "));
    } finally {
      setBusy(false);
    }
  };

  const pair = () =>
    act(async () => {
      const text = await api.phonePair();
      setQr(await QRCode.toDataURL(text, { margin: 1, width: 240, errorCorrectionLevel: "M" }));
      setLeft(PAIR_SECONDS);
    });

  const toggle = (device: string, current: PhonePermission[], p: PhonePermission, on: boolean) =>
    act(() => api.phoneSetPermissions(device, on ? [...current, p] : current.filter((x) => x !== p)));

  if (!status) return null;
  const minutes = `${Math.floor(left / 60)}:${String(left % 60).padStart(2, "0")}`;

  return (
    <FoldCard title={t("phone.title")} open={open} onOpenChange={setOpen} ref={cardRef}>
      <p className="muted small">{t("phone.intro")}</p>
      {error && <div className="banner error">{error}</div>}

      <label className="check">
        <input type="checkbox" checked={status.enabled} disabled={busy} onChange={(e) => act(() => api.phoneSetEnabled(e.target.checked))} />
        <span>
          {t("phone.enable")}
          <span className="muted small block">{t("phone.enableHelp")}</span>
        </span>
      </label>

      {status.enabled && (
        <p className="small phone-card-status">
          <span className={`dot ${status.online ? "dot-success" : ""}`} aria-hidden="true" /> {status.online ? t("phone.online") : t("phone.offline")}
        </p>
      )}

      {pending.map(([device, name]) => (
        <div key={device} className="banner warn phone-ask" role="alert">
          <strong>{t("phone.askTitle", { name })}</strong>
          <p className="small">{t("phone.askHelp")}</p>
          <div className="actions">
            <button className="primary" disabled={busy} onClick={() => act(() => api.phoneAnswer(device, true))}>
              {t("phone.allow")}
            </button>
            <button className="secondary" disabled={busy} onClick={() => act(() => api.phoneAnswer(device, false))}>
              {t("phone.deny")}
            </button>
          </div>
        </div>
      ))}

      {status.enabled && (
        <div className="phone-pair">
          {qr ? (
            <>
              <img src={qr} width={240} height={240} alt={t("phone.qrAlt")} className="phone-qr" />
              <p className="small">{t("phone.scan")}</p>
              <p className="muted small">{t("phone.expires", { time: minutes })}</p>
              <button className="secondary" onClick={() => setQr(null)}>
                {t("phone.cancel")}
              </button>
            </>
          ) : (
            <button className="primary" disabled={busy || !status.online} onClick={pair}>
              {t("phone.pair")}
            </button>
          )}
        </div>
      )}

      {status.phones.length > 0 && <h3 className="bots-heading">{t("phone.paired")}</h3>}
      {status.phones.map((phone) => (
        <div key={phone.device} className="phone-row">
          <div className="phone-head">
            <span className={`dot ${phone.connected ? "dot-success" : ""}`} aria-hidden="true" />
            <strong>{phone.name}</strong>
            <span className="muted small">{phone.connected ? t("phone.connected") : t("phone.notConnected")}</span>
            <button
              className="link danger"
              disabled={busy}
              onClick={() => {
                if (confirm(t("phone.revokeConfirm", { name: phone.name }))) act(() => api.phoneRevoke(phone.device));
              }}
            >
              {t("phone.revoke")}
            </button>
          </div>
          <div className="phone-perms">
            {PHONE_PERMISSIONS.map((p) => (
              <label key={p} className="check small" title={t(`phone.perm.${p}.help` as Key)}>
                <input
                  type="checkbox"
                  checked={phone.permissions.includes(p)}
                  disabled={busy}
                  onChange={(e) => toggle(phone.device, phone.permissions, p, e.target.checked)}
                />
                <span>{t(`phone.perm.${p}` as Key)}</span>
              </label>
            ))}
          </div>
        </div>
      ))}
    </FoldCard>
  );
}
