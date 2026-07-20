import { useEffect, useReducer, useState, type Dispatch } from "react";

import ConnectScreen from "./ConnectScreen.tsx";
import MainScreen from "./MainScreen.tsx";
import { connectHost } from "./connect-host.ts";
import { initialState, reducer, type Action, type AppState } from "./state.ts";

function hasWorkspace(state: AppState): boolean {
  const anyConnected = Object.values(state.fleet).some((entry) => entry.state === "connected");
  return anyConnected || state.sessionOrder.length > 0;
}

export default function App() {
  const [state, dispatch] = useReducer(reducer, undefined, initialState);
  const [showHostManager, setShowHostManager] = useState(false);

  useEffect(() => {
    window.crow
      .fleetList()
      .then((views) => dispatch({ type: "fleet.set", views }))
      .catch(() => undefined);

    // Auto-connect every saved host on startup. hostConnect is idempotent
    // (already-connected returns cached info), so re-runs are cheap.
    window.crow
      .hostsList()
      .then(async (hosts) => {
        dispatch({ type: "hosts.set", hosts });
        await Promise.all(hosts.map((host) => connectHost(host, dispatch)));
      })
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

  const workspace = hasWorkspace(state);

  if (!workspace || showHostManager) {
    return (
      <ConnectScreen
        state={state}
        dispatch={dispatch}
        onClose={workspace ? () => setShowHostManager(false) : undefined}
      />
    );
  }

  return (
    <MainScreen state={state} dispatch={dispatch} onManageHosts={() => setShowHostManager(true)} />
  );
}

export type ScreenProps = {
  state: AppState;
  dispatch: Dispatch<Action>;
};
