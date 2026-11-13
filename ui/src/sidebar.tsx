import { Scroll, signal, Text, token, View } from "nsh";
import { Button } from "nsh/base";
import {
  boot,
  indexProgress,
  indexing,
  leaveNotice,
  navigate,
  openSearch,
  pickAndSwitch,
  quitting,
  sidebarPages,
  switchVault,
  view,
} from "./store";
import { Keycap } from "./widgets";

function basename(path: string): string {
  const parts = path.split("/").filter((p) => p.length > 0);
  return parts[parts.length - 1] ?? path;
}

function SectionLabel(props: { text: string }) {
  return (
    <Text class="text-muted-foreground" style={{ fontSize: 11, paddingHorizontal: 10, marginTop: 18, marginBottom: 4 }}>
      {props.text}
    </Text>
  );
}

/** One line of the sidebar: a page, a view or an action. The dot marks where you are. */
function Row(props: {
  testID?: string;
  label: string;
  onPress: () => void;
  active?: boolean;
  keycap?: string;
}) {
  return (
    <Button
      testID={props.testID}
      onPress={props.onPress}
      class={props.active ? "rounded-md bg-sidebar-accent" : "rounded-md hover:bg-sidebar-accent"}
      style={{ flexDirection: "row", alignItems: "center", minHeight: 32, paddingHorizontal: 10, gap: 8 }}
    >
      <View
        style={{
          width: 5,
          height: 5,
          borderRadius: 3,
          backgroundColor: props.active ? token("seal") : "transparent",
        }}
      />
      <Text class={props.active ? "text-foreground" : "text-sidebar-foreground"} style={{ fontSize: 13 }}>
        {props.label}
      </Text>
      <View style={{ flex: 1 }} />
      <If when={props.keycap}>
        <Keycap label={props.keycap ?? ""} />
      </If>
    </Button>
  );
}

function VaultMenu() {
  const menu = signal(false);
  const others = () => (boot()?.recent ?? []).filter((p) => p !== boot()?.root);
  return (
    <View style={{ flexDirection: "column", marginTop: 10 }}>
      <Button
        testID="vault-menu"
        onPress={() => menu.update((m) => !m)}
        class="rounded-md border border-border bg-card hover:bg-muted"
        style={{ flexDirection: "row", alignItems: "center", minHeight: 28, paddingHorizontal: 8, gap: 6 }}
      >
        <Text class="text-muted-foreground" style={{ fontSize: 12, fontFamily: token("font-mono") }}>
          {basename(boot()?.root ?? "")}
        </Text>
        <View style={{ flex: 1 }} />
        <Text class="text-muted-foreground" style={{ fontSize: 10 }}>
          ▾
        </Text>
      </Button>
      <If when={menu()}>
        <View testID="vault-menu-items" style={{ flexDirection: "column", marginTop: 4 }}>
          <Each of={others()} key={(p) => p}>
            {(path) => (
              <Row
                testID="vault-recent"
                label={basename(path())}
                onPress={() => {
                  menu.set(false);
                  void switchVault(path());
                }}
              />
            )}
          </Each>
          <Row
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
  const onPage = (rel: string) => {
    const v = view();
    return v.kind === "page" && v.rel === rel;
  };
  return (
    <View
      class="border-r border-sidebar-border bg-sidebar"
      style={{
        width: 248,
        flexShrink: 0,
        flexDirection: "column",
        paddingHorizontal: 12,
        paddingTop: 20,
        paddingBottom: 12,
        opacity: quitting() ? 0.5 : 1,
      }}
    >
      <View style={{ flexDirection: "row", alignItems: "center", minHeight: 28, paddingLeft: 10 }}>
        <Text style={{ fontFamily: token("font-serif"), fontSize: 20, letterSpacing: 1.2 }}>日课</Text>
      </View>

      <VaultMenu />

      <SectionLabel text="浏览" />
      <Row
        testID="nav-journals"
        label="日记"
        active={view().kind === "journals"}
        onPress={() => navigate({ kind: "journals" })}
      />
      <Row testID="nav-search" label="搜索" keycap="Ctrl K" onPress={openSearch} />

      <SectionLabel text="页面" />
      <Scroll style={{ flex: 1 }}>
        <View style={{ flexDirection: "column" }}>
          <Each of={sidebarPages()} key={(p) => p.rel}>
            {(page) => (
              <Row
                testID="nav-page"
                label={page().title}
                active={onPage(page().rel)}
                onPress={() => navigate({ kind: "page", rel: page().rel })}
              />
            )}
          </Each>
        </View>
      </Scroll>

      <Text class="text-muted-foreground" style={{ fontSize: 10, paddingHorizontal: 10, marginTop: 6, fontFamily: token("font-mono") }}>
        Ctrl+Shift+V 切库（临时）
      </Text>
      <If when={indexing()}>
        <Text class="text-muted-foreground" style={{ fontSize: 11, paddingHorizontal: 10, marginTop: 6, fontFamily: token("font-mono") }}>
          {`索引 ${indexProgress().done}/${indexProgress().total}`}
        </Text>
      </If>
      <If when={leaveNotice() !== null}>
        <Text testID="leave-notice" class="text-destructive" style={{ fontSize: 11, paddingHorizontal: 10, marginTop: 6 }}>
          {leaveNotice() ?? ""}
        </Text>
      </If>
    </View>
  );
}
