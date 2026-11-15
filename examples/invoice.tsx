// npx papier dev examples/invoice.tsx
// The default export is a React component. Optionally export `options` to set format, margin…

const lines = [
  { label: 'Logo design', qty: 1, price: 450 },
  { label: 'Brand guidelines (20 pages)', qty: 1, price: 1200 },
  { label: 'Social media kit', qty: 6, price: 85.5 },
];
const eur = (n: number) => n.toLocaleString('fr-FR', { style: 'currency', currency: 'EUR' });

export const options = { format: 'A4', margin: '15mm' } as const;

export default function Invoice({ customer = 'Acme Inc.' }: { customer?: string }) {
  const total = lines.reduce((s, l) => s + l.qty * l.price, 0);
  return (
    <div style={{ fontFamily: 'system-ui, sans-serif', color: '#111827' }}>
      <header style={{ display: 'flex', justifyContent: 'space-between', borderBottom: '3px solid #2563eb', paddingBottom: 12 }}>
        <h1 style={{ margin: 0, color: '#2563eb' }}>Invoice #2026-042</h1>
        <div style={{ textAlign: 'right', color: '#6b7280' }}>
          <div>15 Nov 2026</div>
          <div>Due 15 Dec 2026</div>
        </div>
      </header>

      <p style={{ marginTop: 24 }}>
        <strong>Billed to</strong>
        <br />
        {customer}
      </p>

      <table style={{ width: '100%', borderCollapse: 'collapse', marginTop: 16 }}>
        <thead>
          <tr style={{ background: '#2563eb', color: '#fff' }}>
            <th style={{ textAlign: 'left', padding: 8 }}>Description</th>
            <th style={{ padding: 8 }}>Qty</th>
            <th style={{ textAlign: 'right', padding: 8 }}>Unit price</th>
            <th style={{ textAlign: 'right', padding: 8 }}>Total</th>
          </tr>
        </thead>
        <tbody>
          {lines.map((l, i) => (
            <tr key={l.label} style={{ background: i % 2 ? '#eff6ff' : 'transparent' }}>
              <td style={{ padding: 8 }}>{l.label}</td>
              <td style={{ padding: 8, textAlign: 'center' }}>{l.qty}</td>
              <td style={{ padding: 8, textAlign: 'right' }}>{eur(l.price)}</td>
              <td style={{ padding: 8, textAlign: 'right' }}>{eur(l.qty * l.price)}</td>
            </tr>
          ))}
        </tbody>
      </table>

      <p style={{ textAlign: 'right', fontSize: 20, marginTop: 20 }}>
        Total <strong>{eur(total)}</strong>
      </p>
    </div>
  );
}
