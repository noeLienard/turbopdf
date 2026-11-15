// npx tsx examples/simple.ts
import turbopdf from '../src/index';

const pdf = await turbopdf.create('<h1>Invoice</h1><p>Thank you!</p>');
await pdf.save('examples/out/simple.pdf');
console.log(`Saved ${pdf.pages} page(s)`);
