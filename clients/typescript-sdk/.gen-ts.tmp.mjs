import { createClient } from '@hey-api/openapi-ts';
const [input, out] = process.argv.slice(2);
await createClient({
  input, output: { path: out, importFileExtension: '.js' },
  plugins: ['@hey-api/typescript', '@hey-api/sdk', '@hey-api/client-fetch'],
  postProcess: ['prettier'], logs: { level: 'silent' },
});
