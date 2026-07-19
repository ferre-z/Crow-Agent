import { useEffect, useReducer, type Dispatch } from "react";

import ConnectScreen from "./ConnectScreen.tsx";
import MainScreen from "./MainScreen.tsx";
import { initialState, reducer, type Action, type AppState } from "./state.ts";

function hasWorkspace(state: AppState): boolean {
  const anyConnected = Object.values(state.fleet).some((entry) => entry.state === "connected");
  return anyConnected || state.sessionOrder.length > 0;
}

export default function App() {
  const [state, dispatch] = useReducer(reducer, undefined, initialState);

  useEffect(() => {
    window.crow
      .hostsList()
      .then((hosts) => dispatch({ type: "hosts.set", hosts }))
      .catch(() => undefined);

    window.crow
      .fleetList()
      .then((views) => dispatch({ type: "fleet.set", views }))
      .catch(() => undefined);

    const offEvent = window.crow.onDaemonEvent((frame) =>
      dispatch({ type: "daemon.event", frame }),
    );

    const offState = window.crow.onDaemonState((frame) => {
      dispatch({
        type: "fleet.update",
        hostName: frame.hostName,
        patch: { state: frame.state },
      });
      if (frame.state === "connected") {
        window.crow
          .sessionList(frame.hostName)
          .then((sessions) =>
            dispatch({ type: "sessions.set", hostName: frame.hostName, sessions }),
          )
          .catch(() => undefined);
      }
    });

    return () => {
      offEvent();
      offState();
    };
  }, []);

  return hasWorkspace(state) ? (
    <MainScreen state={state} dispatch={dispatch} />
  ) : (
    <ConnectScreen state={state} dispatch={dispatch} />
  );
}

export type ScreenProps = {
  state: AppState;
  dispatch: Dispatch<Action>;
};
