import { createServer } from "node:http";

// An empty, successful response exercises the publisher's unknown-service
// contract without relying on external monitoring infrastructure or targets.
createServer((request, response) => {
  const url = new URL(request.url, "http://prometheus:9090");
  response.writeHead(200, { "Content-Type": "application/json" });
  response.end(
    JSON.stringify({
      status: "success",
      data: {
        // Range selectors (e.g. `probe_success{…}[1h]`) return raw samples.
        resultType:
          url.pathname.endsWith("query_range") ||
          /\]$/.test(url.searchParams.get("query") ?? "")
            ? "matrix"
            : "vector",
        result: [],
      },
    }),
  );
}).listen(9090, "0.0.0.0");
