import {
  errorEnvelopeSchema,
  pluginToHostMessageSchema,
  themeDefinitionSchema,
  type ErrorEnvelope,
  type PluginDescriptor,
  type RpcResult,
  type ThemeDefinition
} from "@git-ramus/contracts";
import { useEffect, useRef, useState } from "react";
import type { HostApi } from "../lib/hostApi";
import { dispatchPluginRpc } from "./rpcRouter";

interface PluginFrameProps {
  descriptor: PluginDescriptor;
  hostApi: HostApi;
  route?: string;
  theme?: ThemeDefinition | null;
}

export function PluginFrame({ descriptor, hostApi, route = "/", theme = null }: PluginFrameProps) {
  const frameRef = useRef<HTMLIFrameElement>(null);
  const readyRef = useRef(false);
  const initializedRef = useRef(false);
  const [sessionId] = useState(() => crypto.randomUUID());
  const [bridgeStatus, setBridgeStatus] = useState<
    "loading" | "ready" | "rpc-complete" | "rpc-failed"
  >("loading");

  useEffect(() => {
    const receive = (event: MessageEvent<unknown>) => {
      if (event.source !== frameRef.current?.contentWindow) {
        return;
      }
      const parsed = pluginToHostMessageSchema.safeParse(event.data);
      if (!parsed.success || parsed.data.sessionId !== sessionId) {
        return;
      }
      if (parsed.data.type === "plugin:ready") {
        readyRef.current = true;
        setBridgeStatus("ready");
        return;
      }
      if (parsed.data.type === "rpc:request") {
        const request = parsed.data;
        const isHandshakeRpc = readyRef.current && request.method === "app.getInfo";
        void dispatchPluginRpc(descriptor.manifest.id, request, hostApi)
          .then((result) => {
            if (isHandshakeRpc) {
              setBridgeStatus("rpc-complete");
            }
            postResult(frameRef.current, {
              type: "rpc:result",
              requestId: request.requestId,
              sessionId,
              ok: true,
              result
            });
          })
          .catch((error: unknown) => {
            if (isHandshakeRpc) {
              setBridgeStatus("rpc-failed");
            }
            postResult(frameRef.current, {
              type: "rpc:result",
              requestId: request.requestId,
              sessionId,
              ok: false,
              error: toPluginError(error, descriptor.manifest.id)
            });
          });
      }
    };
    window.addEventListener("message", receive);
    return () => window.removeEventListener("message", receive);
  }, [descriptor.manifest.id, hostApi, sessionId]);

  useEffect(() => {
    if (!initializedRef.current) {
      return;
    }
    postTheme(frameRef.current, sessionId, theme);
  }, [sessionId, theme]);

  const initialize = () => {
    readyRef.current = false;
    initializedRef.current = true;
    setBridgeStatus("loading");
    frameRef.current?.contentWindow?.postMessage(
      {
        type: "host:init",
        sessionId,
        pluginId: descriptor.manifest.id,
        sdkVersion: "0.1.0",
        route
      },
      "*"
    );
    postTheme(frameRef.current, sessionId, theme);
  };

  return (
    <iframe
      ref={frameRef}
      title={`${descriptor.manifest.name} plugin`}
      sandbox="allow-scripts"
      src={descriptor.uiUrl}
      data-plugin-status={bridgeStatus}
      onLoad={initialize}
    />
  );
}

function postResult(frame: HTMLIFrameElement | null, result: RpcResult) {
  frame?.contentWindow?.postMessage(result, "*");
}

function postTheme(
  frame: HTMLIFrameElement | null,
  sessionId: string,
  theme: ThemeDefinition | null
) {
  if (theme === null) {
    return;
  }
  const parsed = themeDefinitionSchema.safeParse(theme);
  if (!parsed.success) {
    return;
  }
  frame?.contentWindow?.postMessage(
    {
      type: "host:theme-changed",
      sessionId,
      theme: parsed.data
    },
    "*"
  );
}

function toPluginError(error: unknown, pluginId: string): ErrorEnvelope {
  const structured = errorEnvelopeSchema.safeParse(error);
  if (structured.success) {
    return structured.data;
  }
  return {
    code: "plugin.rpc-failed",
    category: "internalFatal",
    message: "Plugin RPC failed",
    operationId: null,
    pluginId,
    resourceId: null,
    failedStep: "rpc.dispatch",
    retryable: false,
    retryAfterMs: null,
    recoveryActions: [],
    details: null
  };
}
