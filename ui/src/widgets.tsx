import { Text, token, View } from "nsh";
import { Button } from "nsh/base";

/** A small text button: a quiet box, used for the few actions that are not navigation. */
export function Btn(props: { label: string; onPress: () => void; testID?: string; disabled?: boolean }) {
  return (
    <Button
      testID={props.testID}
      disabled={props.disabled}
      onPress={props.onPress}
      class="rounded-md border border-border bg-card px-2.5 py-1 hover:bg-muted"
      disabledStyle={{ opacity: 0.4 }}
    >
      <Text class="text-foreground" style={{ fontSize: 12 }}>
        {props.label}
      </Text>
    </Button>
  );
}

/** A shortcut, shown where the action is. */
export function Keycap(props: { label: string }) {
  return (
    <View class="rounded border border-border bg-card" style={{ paddingHorizontal: 4, paddingVertical: 1 }}>
      <Text class="text-muted-foreground" style={{ fontSize: 10, fontFamily: token("font-mono"), letterSpacing: 0.3 }}>
        {props.label}
      </Text>
    </View>
  );
}

/** A glyph in a square: back, forward, close. */
export function GlyphButton(props: {
  testID: string;
  glyph: string;
  label: string;
  onPress: () => void;
  disabled?: boolean;
}) {
  return (
    <Button
      testID={props.testID}
      disabled={props.disabled}
      onPress={props.onPress}
      accessibilityLabel={props.label}
      class="rounded-md hover:bg-muted"
      disabledStyle={{ opacity: 0.3 }}
      style={{ width: 26, minHeight: 26, alignItems: "center", justifyContent: "center" }}
    >
      <Text class="text-muted-foreground" style={{ fontSize: 14 }}>
        {props.glyph}
      </Text>
    </Button>
  );
}
