---
type: rdp-note
tags:
  - rdp
  - firefox-client
  - flow
date: 2026-05-23
firefox_files:
  - devtools/shared/transport/transport.js
  - devtools/client/devtools-client.js
  - devtools/shared/specs/root.js
  - devtools/client/fronts/root.js
title: "Flow: Connect and list tabs"
---

# Flow: Connect and list tabs

The canonical "hello world" of RDP. This is what DevTools' about:debugging
page does first, and what ff-rdp does at the start of nearly every command.

## Step-by-step

1. **Open TCP socket** to `localhost:6000` (or wherever
   `--start-debugger-server` was set). Firefox-side accept happens in the
   socket actor; from the client side this is just an `nsITransportService`
   or, for ff-rdp, a Rust `tokio::net::TcpStream`.

2. **Wrap the stream in a transport.** In Firefox DevTools:

   ```js
   const transport = new DebuggerTransport(input, output);
   const client = new DevToolsClient(transport);
   ```

   Constructor (`devtools-client.js:75-121`) immediately sets
   `transport.hooks = this` and registers `expectReply("root", cb)` — the
   listener for the server greeting. **Then** the client awaits the actual
   handshake.

3. **Start the transport** and read the greeting. `client.connect()`
   (`devtools-client.js:146-156`) calls `transport.ready()`, which begins
   reading bytes. The first complete JSON packet the client receives is:

   ```json
   {
     "from": "root",
     "applicationType": "browser",
     "testConnectionPrefix": "server1.conn0.",
     "traits": { "watcher": true, "networkMonitor": true, ... }
   }
   ```

   `traits` is a feature-flag bag — clients use it to decide e.g. whether
   to use the new watcher-based resource API.

4. **Build the RootFront** from the greeting:
   `this.mainRoot = createRootFront(this, packet)`. Then call the new
   `RootFront.connect({frontendVersion})` request (Fx 133+) to tell the
   server who we are. Emit `"connected"` so the outer `client.connect()`
   promise resolves.

5. **Call `mainRoot.listTabs()`.** Per `specs/root.js:38-43`:

   ```js
   listTabs: {
     request: {},
     response: { tabs: RetVal("array:tabDescriptor") },
   }
   ```

   Wire request:

   ```json
   {"to": "root", "type": "listTabs"}
   ```

   Wire reply (one entry shown):

   ```json
   {
     "from": "root",
     "tabs": [
       {
         "actor": "server1.conn0.tabDescriptor3",
         "browserId": 17,
         "browsingContextID": 12,
         "outerWindowID": 8589934592,
         "url": "https://example.com/",
         "title": "Example Domain",
         "selected": true,
         "isZombieTab": false,
         "traits": { ... }
       }
     ]
   }
   ```

   The `array:tabDescriptor` return type means each entry is unmarshalled
   into a `TabDescriptorFront`, registered in the connection's actor pool,
   and addressable by the actorID in the `actor` field. (ff-rdp doesn't do
   this — it just keeps the JSON.)

## What the client now has

- `mainRoot.actorID === "root"`.
- An array of `TabDescriptorFront` (or in ff-rdp: JSON objects) with
  metadata. **The descriptor is not yet "attached"** — you can read URL/title
  but cannot evaluate JS, take screenshots, or watch resources until you
  promote it to a target (see [[attach-target]]).

## ff-rdp implementation pointers

ff-rdp packs the above into a single helper in `crates/ff-rdp-core/src/client.rs`
(the `connect()` constructor reads the greeting and the transport
loop dispatches subsequent replies by `from`-actor). `ff-rdp tabs list` is just
"connect → send `{to:root, type:listTabs}` → return tabs array".

Next: [[attach-target]].
