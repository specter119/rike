import { mount, Text, View } from "nsh";
import { Button } from "nsh/base";
import { CurrentView } from "./src/page";
import { SearchOverlay } from "./src/search";
import { Sidebar } from "./src/sidebar";
import { installShortcuts } from "./src/shortcuts";
import { Toasts } from "./src/notices";
import { init, mode, quit, quitting } from "./src/store";
import { Welcome } from "./src/welcome";

/** The window has no system title bar on every desktop, so it brings its own close button. */
function CloseBar() {
  return (
    <View style={{ flexDirection: "row", justifyContent: "flex-end", paddingHorizontal: 6, paddingVertical: 2 }}>
      <Button
        testID="window-close"
        onPress={() => void quit()}
        class="rounded px-2 py-1 hover:bg-muted"
        accessibilityLabel="关闭"
      >
        <Text class="text-muted-foreground" style={{ fontSize: 13 }}>
          ✕
        </Text>
      </Button>
    </View>
  );
}

function App() {
  installShortcuts();
  void init();
  return (
    <View class="bg-background" style={{ flex: 1, flexDirection: "column" }}>
      <CloseBar />
      <View style={{ flex: 1, flexDirection: "row" }}>
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
    </View>
  );
}

mount(App);
