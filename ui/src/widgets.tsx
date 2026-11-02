import { Text } from "nsh";
import { Button } from "nsh/base";

/** A small text button. */
export function Btn(props: { label: string; onPress: () => void; testID?: string; disabled?: boolean }) {
  return (
    <Button
      testID={props.testID}
      disabled={props.disabled}
      onPress={props.onPress}
      class="rounded bg-muted px-2 py-1"
      disabledStyle={{ opacity: 0.4 }}
    >
      <Text class="text-foreground" style={{ fontSize: 12 }}>
        {props.label}
      </Text>
    </Button>
  );
}
