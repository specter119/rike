import { Text, token, View } from "nsh";
import type { JSX } from "nsh";
import { Button } from "nsh/base";
import { Window } from "nsh/desktop";
import { quit } from "./store";

/**
 * The window has no system title bar, so the app draws the controls the
 * window manager would: minimize, maximize, close, in the top right corner.
 */
function Control(props: { testID: string; label: string; onPress: () => void; children: JSX.Element }) {
  return (
    <Button
      testID={props.testID}
      onPress={props.onPress}
      accessibilityLabel={props.label}
      class="rounded-md hover:bg-muted"
      style={{ width: 26, minHeight: 26, alignItems: "center", justifyContent: "center" }}
    >
      {props.children}
    </Button>
  );
}

export function WindowControls() {
  const win = () => Window.get("main");
  const maximized = () => win()?.state()?.mode === "maximized";
  return (
    <View style={{ flexDirection: "row", alignItems: "center", gap: 2 }}>
      <Control testID="window-minimize" label="最小化" onPress={() => win()?.minimize()}>
        <View style={{ width: 10, height: 1.5, borderRadius: 1, backgroundColor: token("muted-foreground") }} />
      </Control>
      <Control
        testID="window-maximize"
        label={maximized() ? "还原" : "最大化"}
        onPress={() => win()?.setMode(maximized() ? "normal" : "maximized")}
      >
        <View
          style={{
            width: 9,
            height: 9,
            borderWidth: 1.2,
            borderRadius: 1.5,
            borderColor: token("muted-foreground"),
          }}
        />
      </Control>
      <Control testID="window-close" label="关闭" onPress={() => void quit()}>
        <Text class="text-muted-foreground" style={{ fontSize: 13 }}>
          ✕
        </Text>
      </Control>
    </View>
  );
}
