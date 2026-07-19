import { useEffect, useReducer, type Dispatch } from "react";

import ConnectScreen from "./ConnectScreen.tsx";
import MainScreen from "./MainScreen.tsx";
import { initialState, reducer, type Action, type AppState } from "./state.ts";

export default function App() {
  const [state, dispatch] = useReducer(reducer, undefined, initialState);

  useEffect(() => {
    window.crow
      .hostsList()
      .then((hosts) => dispatch({ type: "hosts.set", hosts }))
      .catch(() => undefined);
    const offEvent = window.crow.onDaemonEvent((frame) =>
      dispatch({ type: "daemon.event", frame }),
    );
    const offState = window.crow.onDaemonState((connectionState) =>
      dispatch({ type: "daemon.connection", state: connectionState }),
    );
    return () => {
      offEvent();
      offState();
    };
  }, []);

  return state.connection === "connected" ? (
    <MainScreen state={state} dispatch={dispatch} />
  ) : (
    <ConnectScreen state={state} dispatch={dispatch} />
  );
}

export type ScreenProps = {
  state: AppState;
  dispatch: Dispatch<Action>;
};
