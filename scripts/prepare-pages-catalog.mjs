import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";

const distDir = path.resolve("dist");
const catalogPath = path.join(distDir, "catalog.db");
const manifestPath = path.join(distDir, "catalog.db.parts.json");
const chunkSizeBytes = 20 * 1024 * 1024;

async function main() {
  let bytes;
  try {
    bytes = await readFile(catalogPath);
  } catch (error) {
    if (error && typeof error === "object" && "code" in error && error.code === "ENOENT") {
      return;
    }
    throw error;
  }

  if (bytes.byteLength <= chunkSizeBytes) {
    return;
  }

  await mkdir(distDir, { recursive: true });

  const parts = [];
  let index = 0;
  for (let offset = 0; offset < bytes.byteLength; offset += chunkSizeBytes) {
    index += 1;
    const filename = `catalog.db.part${String(index).padStart(3, "0")}`;
    const chunk = bytes.subarray(offset, offset + chunkSizeBytes);
    await writeFile(path.join(distDir, filename), chunk);
    parts.push(filename);
  }

  await writeFile(
    manifestPath,
    `${JSON.stringify({ parts }, null, 2)}\n`,
  );
  await rm(catalogPath);
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
