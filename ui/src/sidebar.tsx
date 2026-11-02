import { signal, Scroll, Text, View } from "nsh";
import { Button } from "nsh/base";
import {
  back,
  boot,
  canBack,
  canForward,
  forward,
  leaveNotice,
  navigate,
  openSearch,
  pickAndSwitch,
  quit,
  quitting,
  sidebarPages,
  switchVault,
} from "./store";
import { Btn } from "./widgets";

function basename(path: string): string {
  const parts = path.split("/").filter((p) => p.length > 0);
  return parts[parts.length - 1] ?? path;
}

function Nav(props: { testID: string; label: string; onPress: () => void }) {
  return (
    <Button testID={props.testID} onPress={props.onPress} class="rounded" style={{ paddingHorizontal: 8, paddingVertical: 4 }}>
      <Text class="text-sidebar-foreground" style={{ fontSize: 13 }}>
        {props.label}
      </Text>
    </Button>
  );
}

function VaultMenu() {
  const menu = signal(false);
  const others = () => (boot()?.recent ?? []).filter((p) => p !== boot()?.root);
  return (
    <View style={{ flexDirection: "column", gap: 2 }}>
      <Nav testID="vault-menu" label={`${basename(boot()?.root ?? "")} ▾`} onPress={() => menu.update((m) => !m)} />
      <If when={menu()}>
        <View testID="vault-menu-items" style={{ flexDirection: "column", gap: 2, paddingLeft: 8 }}>
          <Each of={others()} key={(p) => p}>
            {(path) => (
              <Nav
                testID="vault-recent"
                label={basename(path())}
                onPress={() => {
                  menu.set(false);
                  void switchVault(path());
                }}
              />
            )}
          </Each>
          <Nav
            testID="vault-open-folder"
            label="打开文件夹…"
            onPress={() => {
              menu.set(false);
              void pickAndSwitch();
            }}
          />
        </View>
      </If>
    </View>
  );
}

export function Sidebar() {
  return (
    <View
      class="border-r border-sidebar-border bg-sidebar"
      style={{ width: 220, flexShrink: 0, flexDirection: "column", padding: 8, gap: 4, opacity: quitting() ? 0.5 : 1 }}
    >
      <Text class="text-sidebar-foreground" style={{ fontSize: 16, fontWeight: "600" }}>
        日课
      </Text>
      <VaultMenu />
      <View style={{ flexDirection: "row", gap: 4 }}>
        <Btn testID="nav-back" label="←" disabled={!canBack()} onPress={back} />
        <Btn testID="nav-forward" label="→" disabled={!canForward()} onPress={forward} />
      </View>
      <Nav testID="nav-journals" label="Journals" onPress={() => navigate({ kind: "journals" })} />
      <Nav testID="nav-search" label="搜索  Ctrl+K" onPress={openSearch} />
      <Text class="text-muted-foreground" style={{ fontSize: 11, marginTop: 8 }}>
        所有页面
      </Text>
      <Scroll style={{ flex: 1 }}>
        <View style={{ flexDirection: "column" }}>
          <Each of={sidebarPages()} key={(p) => p.rel}>
            {(page) => <Nav testID="nav-page" label={page().title} onPress={() => navigate({ kind: "page", rel: page().rel })} />}
          </Each>
        </View>
      </Scroll>
      <If when={leaveNotice() !== null}>
        <Text testID="leave-notice" class="text-destructive" style={{ fontSize: 11 }}>
          {leaveNotice() ?? ""}
        </Text>
      </If>
      <Nav testID="quit" label="退出  Ctrl+W" onPress={() => void quit()} />
    </View>
  );
}
