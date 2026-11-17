import { useStore } from "../hooks/useStore";
import { closeAddWorkspace, closePluginUpdate } from "../app/actions/settings";
import { workspaceAdded } from "../app/actions/workspaces";
import { flushBeforeExit } from "../app/lifecycle";
import { uiStore } from "../stores/ui";
import AddWorkspaceDialog from "./AddWorkspaceDialog";
import NoticeCenter from "./NoticeCenter";
import PluginUpdateDialog from "./PluginUpdateDialog";
import SettingsDialog from "./SettingsDialog";
import WorkspaceLocationDialog from "./WorkspaceLocationDialog";

/** Avvisi e dialoghi dell'app: ognuno legge il proprio stato dagli archivi. */
export default function AppDialogs() {
  const addWorkspaceOpen = useStore(uiStore, (s) => s.addWorkspaceOpen);
  const outdatedPlugins = useStore(uiStore, (s) => s.outdatedPlugins);

  return (
    <>
      <NoticeCenter />
      <AddWorkspaceDialog isOpen={addWorkspaceOpen} onClose={closeAddWorkspace} onAdded={workspaceAdded} />
      <PluginUpdateDialog outdated={outdatedPlugins} onClose={closePluginUpdate} onBeforeRestart={flushBeforeExit} />
      <SettingsDialog />
      <WorkspaceLocationDialog />
    </>
  );
}
