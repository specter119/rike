import { Text, token, View } from "nsh";
import { Button } from "nsh/base";
import { boot, confirmOpen, leaveNotice, pendingConfirm, pickAndSwitch, startupError, switchVault } from "./store";
import { WindowControls } from "./window";
import { Btn } from "./widgets";

export function Welcome() {
  return (
    <View style={{ flex: 1, flexDirection: "column" }}>
      <View style={{ flexDirection: "row", justifyContent: "flex-end", paddingTop: 10, paddingHorizontal: 10 }}>
        <WindowControls />
      </View>
      <View testID="welcome" style={{ flex: 1, alignItems: "center", justifyContent: "center", padding: 24 }}>
        <View style={{ width: "100%", maxWidth: 520, flexDirection: "column", gap: 12 }}>
        <View style={{ flexDirection: "column", gap: 6, marginBottom: 8 }}>
          <Text style={{ fontFamily: token("font-serif"), fontSize: 30, letterSpacing: 1.5 }}>日课</Text>
          <Text class="text-muted-foreground" style={{ fontSize: 11, fontFamily: token("font-mono"), letterSpacing: 0.4 }}>
            RIKE · 本地优先的大纲笔记
          </Text>
        </View>
        <Text class="text-muted-foreground" style={{ fontSize: 13 }}>
          选择一个笔记库文件夹。Logseq 的文件夹可以直接打开。
        </Text>
        <If when={boot()?.notice}>
          {(n) => (
            <Text testID="welcome-notice" class="text-destructive" style={{ fontSize: 13 }}>
              {n()}
            </Text>
          )}
        </If>
        <If when={startupError() !== null}>
          <Text testID="startup-error" class="text-destructive" style={{ fontSize: 13 }}>
            {startupError() ?? ""}
          </Text>
        </If>
        <If when={leaveNotice() !== null}>
          <Text testID="leave-notice" class="text-destructive" style={{ fontSize: 13 }}>
            {leaveNotice() ?? ""}
          </Text>
        </If>
        <If when={pendingConfirm()}>
          {(p) => (
            <View testID="confirm" class="rounded-md border border-border bg-muted" style={{ flexDirection: "column", gap: 8, padding: 12 }}>
              <Text style={{ fontSize: 13 }}>{`${p().path} 不像笔记库（里面有别的文件）。仍然把它当作笔记库打开吗？`}</Text>
              <View style={{ flexDirection: "row", gap: 8 }}>
                <Btn testID="confirm-yes" label="打开" onPress={() => confirmOpen(true)} />
                <Btn testID="confirm-no" label="取消" onPress={() => confirmOpen(false)} />
              </View>
            </View>
          )}
        </If>
        <View style={{ flexDirection: "row", gap: 8 }}>
          <Btn testID="open-folder" label="打开文件夹…" onPress={() => void pickAndSwitch()} />
        </View>
        <If when={(boot()?.recent.length ?? 0) > 0}>
          <Text class="text-muted-foreground" style={{ fontSize: 11, marginTop: 12 }}>
            最近打开
          </Text>
        </If>
        <Each of={boot()?.recent ?? []} key={(p) => p}>
          {(path) => (
            <Button
              testID="recent-vault"
              onPress={() => void switchVault(path())}
              class="rounded-md border border-border bg-card hover:bg-muted"
              style={{ padding: 8 }}
            >
              <Text style={{ fontSize: 12, fontFamily: token("font-mono") }}>{path()}</Text>
            </Button>
          )}
        </Each>
        </View>
      </View>
    </View>
  );
}
