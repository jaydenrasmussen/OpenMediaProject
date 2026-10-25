"use client";

import { useState } from "react";
import { type Key, t } from "../lib/i18n";
import { CustomFormats } from "./CustomFormats";
import { DownloadClients } from "./DownloadClients";
import { General } from "./General";
import { Indexers } from "./Indexers";
import { Interface } from "./Interface";
import { Lists } from "./Lists";
import { MediaManagement } from "./MediaManagement";
import { MediaServers } from "./MediaServers";
import { Notifications } from "./Notifications";
import { Plugins } from "./Plugins";
import { Profiles } from "./Profiles";
import { QualitySizes } from "./QualitySizes";
import { Rules } from "./Rules";
import { Vpn } from "./Vpn";

const TABS: { id: string; label: Key; view: () => React.ReactNode }[] = [
  { id: "media", label: "settings.media", view: MediaManagement },
  { id: "profiles", label: "settings.profiles", view: Profiles },
  { id: "formats", label: "settings.formats", view: CustomFormats },
  { id: "sizes", label: "settings.sizes", view: QualitySizes },
  { id: "rules", label: "settings.rules", view: Rules },
  { id: "indexers", label: "settings.indexers", view: Indexers },
  { id: "clients", label: "settings.clients", view: DownloadClients },
  { id: "lists", label: "settings.lists", view: Lists },
  { id: "mediaservers", label: "settings.mediaServers", view: MediaServers },
  { id: "notifications", label: "settings.notifications", view: Notifications },
  { id: "plugins", label: "settings.plugins", view: Plugins },
  { id: "vpn", label: "settings.vpn", view: Vpn },
  { id: "ui", label: "settings.ui", view: Interface },
  { id: "general", label: "settings.general", view: General },
];

export default function Settings() {
  const [current, setCurrent] = useState("media");
  const View = TABS.find((x) => x.id === current)!.view;
  return (
    <>
      <h1>{t("settings.title")}</h1>
      <div className="tabs" role="tablist">
        {TABS.map((x) => (
          <button key={x.id} role="tab" className={current === x.id ? "active" : ""} onClick={() => setCurrent(x.id)}>
            {t(x.label)}
          </button>
        ))}
      </div>
      <View />
    </>
  );
}
