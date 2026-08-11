#!/usr/bin/env node
import { createServer } from "./server.js";

const port = Number(process.env.PORT) || 3000;

createServer().listen(port, () => {
  console.log(`compyla-server listening on http://localhost:${port}`);
});
