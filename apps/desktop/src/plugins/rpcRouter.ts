import type { RpcRequest } from "@git-ramus/contracts";
import type { HostApi } from "../lib/hostApi";

interface Route {
  capability: string;
  resource: string;
  handle(pluginId: string, params: unknown, hostApi: HostApi): Promise<unknown>;
}

const routes: Record<string, Route> = {
  "app.getInfo": {
    capability: "app:read",
    resource: "info",
    handle: (_pluginId, _params, hostApi) => hostApi.getAppInfo()
  },
  "tasks.startEcho": {
    capability: "tasks:create",
    resource: "echo",
    async handle(pluginId, params, hostApi) {
      if (!isEchoParams(params)) {
        throw new Error("tasks.startEcho requires a non-empty message");
      }
      return hostApi.startEchoJob(pluginId, params.message);
    }
  }
};

export async function dispatchPluginRpc(
  pluginId: string,
  request: RpcRequest,
  hostApi: HostApi
): Promise<unknown> {
  const route = routes[request.method];
  if (route === undefined) {
    throw new Error(`Unknown plugin RPC method: ${request.method}`);
  }
  const decision = await hostApi.authorizePluginCall({
    pluginId,
    capability: route.capability,
    resource: route.resource
  });
  if (!decision.allowed) {
    throw new Error(`Permission denied: ${route.capability}/${route.resource}`);
  }
  return route.handle(pluginId, request.params, hostApi);
}

function isEchoParams(value: unknown): value is { message: string } {
  return (
    typeof value === "object" &&
    value !== null &&
    "message" in value &&
    typeof value.message === "string" &&
    value.message.trim().length > 0
  );
}
