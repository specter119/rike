import { mount, Text, View } from "nsh";
import { CurrentView } from "./src/page";
import { SearchOverlay } from "./src/search";
import { Sidebar } from "./src/sidebar";
import { installShortcuts } from "./src/shortcuts";
import { Toasts } from "./src/notices";
import { init, mode, quitting } from "./src/store";
import { Welcome } from "./src/welcome";

function App() {
  installShortcuts();
  void init();
  return (
    <View class="bg-background" style={{ flex: 1, flexDirection: "row" }}>
      <If when={mode() === "loading"}>
        <View style={{ flex: 1, alignItems: "center", justifyContent: "center" }}>
          <Text class="text-muted-foreground">Rike</Text>
        </View>
      </If>
      <If when={mode() === "welcome"}>
        <Welcome />
      </If>
      <If when={mode() === "vault"}>
        <Sidebar />
        <View style={{ flex: 1, flexDirection: "column", opacity: quitting() ? 0.5 : 1 }}>
          <Toasts />
          <SearchOverlay />
          <CurrentView />
        </View>
      </If>
    </View>
  );
}

mount(App);
