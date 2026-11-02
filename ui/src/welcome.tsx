import { Text, View } from "nsh";
import { Button } from "nsh/base";
import { boot, confirmOpen, leaveNotice, pendingConfirm, pickAndSwitch, quit, startupError, switchVault } from "./store";
import { Btn } from "./widgets";

export function Welcome() {
  return (
    <View testID="welcome" style={{ flex: 1, alignItems: "center", justifyContent: "center", padding: 24 }}>
      <View style={{ width: "100%", maxWidth: 520, flexDirection: "column", gap: 12 }}>
        <Text style={{ fontSize: 28, fontWeight: "600" }}>Rike（日课）</Text>
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
            <View testID="confirm" class="rounded-md bg-muted" style={{ flexDirection: "column", gap: 8, padding: 12 }}>
              <Text style={{ fontSize: 13 }}>{`${p().path} 不像笔记库（里面有别的文件）。仍然把它当作笔记库打开吗？`}</Text>
              <View style={{ flexDirection: "row", gap: 8 }}>
                <Btn testID="confirm-yes" label="打开" onPress={() => confirmOpen(true)} />
                <Btn testID="confirm-no" label="取消" onPress={() => confirmOpen(false)} />
              </View>
            </View>
          )}
        </If>
        <Btn testID="open-folder" label="打开文件夹…" onPress={() => void pickAndSwitch()} />
        <If when={(boot()?.recent.length ?? 0) > 0}>
          <Text class="text-muted-foreground" style={{ fontSize: 12, marginTop: 8 }}>
            最近打开
          </Text>
        </If>
        <Each of={boot()?.recent ?? []} key={(p) => p}>
          {(path) => (
            <Button testID="recent-vault" onPress={() => void switchVault(path())} class="rounded bg-muted" style={{ padding: 8 }}>
              <Text style={{ fontSize: 13 }}>{path()}</Text>
            </Button>
          )}
        </Each>
        <Btn testID="quit" label="退出" onPress={() => void quit()} />
      </View>
    </View>
  );
}
