// An invoice written in React with plain inline styles. Rendered by examples/showcase/render.tsx.

const items = [
  { name: 'Product design sprint', detail: 'Research, wireframes, 2 rounds of user tests', qty: 1, price: 6400 },
  { name: 'Design system', detail: '48 components, tokens, Figma library', qty: 1, price: 4800 },
  { name: 'Marketing website', detail: '6 pages, responsive, CMS integration', qty: 1, price: 5200 },
  { name: 'Illustrations', detail: 'Custom spot illustrations', qty: 12, price: 180 },
  { name: 'Motion design', detail: 'Product walkthrough, 45 seconds', qty: 1, price: 1900 },
  { name: 'Support retainer', detail: 'November 2026, hours', qty: 14, price: 120 },
];

const usd = (n: number) => n.toLocaleString('en-US', { style: 'currency', currency: 'USD' });
const subtotal = items.reduce((s, i) => s + i.qty * i.price, 0);
const tax = subtotal * 0.08;
const total = subtotal + tax;

const ink = '#0f172a';
const muted = '#64748b';
const accent = '#6366f1';
const line = '#e2e8f0';

const Label = ({ children }: { children: string }) => (
  <div style={{ fontSize: 9, fontWeight: 600, color: muted, marginBottom: 6 }}>{children}</div>
);

export default function Invoice() {
  return (
    <div style={{ fontFamily: 'Inter', color: ink, fontSize: 11 }}>
      {/* header band */}
      <div style={{ backgroundColor: ink, color: '#fff', padding: '44px 56px 40px', display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
          <svg width="34" height="34" viewBox="0 0 34 34">
            <rect width="34" height="34" rx="9" fill={accent} />
            <path d="M10 23 L17 9 L24 23 Z" fill="#fff" />
            <circle cx="17" cy="20" r="3" fill={accent} />
          </svg>
          <div>
            <div style={{ fontSize: 16, fontWeight: 700 }}>Northwind Studio</div>
            <div style={{ fontSize: 10, color: '#94a3b8' }}>Design & product engineering</div>
          </div>
        </div>
        <div style={{ textAlign: 'right' }}>
          <div style={{ fontSize: 30, fontWeight: 700 }}>Invoice</div>
          <div style={{ fontSize: 11, color: '#a5b4fc', fontWeight: 600 }}>#NW-2026-1142</div>
        </div>
      </div>

      <div style={{ padding: '36px 56px 0' }}>
        {/* parties and dates */}
        <div style={{ display: 'grid', gridTemplateColumns: '1.3fr 1.3fr 1fr', gap: 24 }}>
          <div>
            <Label>BILLED TO</Label>
            <div style={{ fontWeight: 700, fontSize: 12 }}>Acme Robotics, Inc.</div>
            <div style={{ color: muted, lineHeight: 1.5 }}>Attn. Jordan Lee<br />500 Market Street, Suite 210<br />San Francisco, CA 94105</div>
          </div>
          <div>
            <Label>FROM</Label>
            <div style={{ fontWeight: 700, fontSize: 12 }}>Northwind Studio LLC</div>
            <div style={{ color: muted, lineHeight: 1.5 }}>88 Harbor Lane<br />Portland, OR 97209<br />hello@northwind.studio</div>
          </div>
          <div>
            <Label>ISSUED</Label>
            <div style={{ fontWeight: 600, marginBottom: 10 }}>Nov 15, 2026</div>
            <Label>DUE</Label>
            <div style={{ fontWeight: 600 }}>Dec 15, 2026</div>
          </div>
        </div>

        {/* items */}
        <table style={{ width: '100%', borderCollapse: 'collapse', marginTop: 34 }}>
          <thead>
            <tr>
              <th style={{ textAlign: 'left', padding: '10px 12px', backgroundColor: '#f1f5f9', fontSize: 9, color: muted }}>DESCRIPTION</th>
              <th style={{ textAlign: 'right', padding: '10px 12px', backgroundColor: '#f1f5f9', fontSize: 9, color: muted, width: 50 }}>QTY</th>
              <th style={{ textAlign: 'right', padding: '10px 12px', backgroundColor: '#f1f5f9', fontSize: 9, color: muted, width: 90 }}>RATE</th>
              <th style={{ textAlign: 'right', padding: '10px 12px', backgroundColor: '#f1f5f9', fontSize: 9, color: muted, width: 100 }}>AMOUNT</th>
            </tr>
          </thead>
          <tbody>
            {items.map((i, n) => (
              <tr key={i.name} style={{ backgroundColor: n % 2 ? '#f8fafc' : '#ffffff' }}>
                <td style={{ padding: '12px' }}>
                  <div style={{ fontWeight: 600 }}>{i.name}</div>
                  <div style={{ color: muted, fontSize: 9.5, marginTop: 2 }}>{i.detail}</div>
                </td>
                <td style={{ textAlign: 'right', padding: '12px' }}>{i.qty}</td>
                <td style={{ textAlign: 'right', padding: '12px', color: muted }}>{usd(i.price)}</td>
                <td style={{ textAlign: 'right', padding: '12px', fontWeight: 600 }}>{usd(i.qty * i.price)}</td>
              </tr>
            ))}
          </tbody>
        </table>

        {/* payment + totals */}
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', marginTop: 28, gap: 32 }}>
          <div style={{ display: 'flex', gap: 14, alignItems: 'center', border: `1px solid ${line}`, borderRadius: 10, padding: 14 }}>
            <img src="qr:https://pay.northwind.studio/NW-2026-1142" alt="Pay this invoice online" width={76} height={76} />
            <div>
              <div style={{ fontWeight: 700, marginBottom: 4 }}>Scan to pay</div>
              <div style={{ color: muted, fontSize: 9.5, lineHeight: 1.5 }}>Card, ACH or wire transfer<br />pay.northwind.studio</div>
            </div>
          </div>
          <div style={{ width: 230 }}>
            <div style={{ display: 'flex', justifyContent: 'space-between', padding: '4px 0' }}>
              <span style={{ color: muted }}>Subtotal</span><span>{usd(subtotal)}</span>
            </div>
            <div style={{ display: 'flex', justifyContent: 'space-between', padding: '4px 0 10px' }}>
              <span style={{ color: muted }}>Sales tax (8%)</span><span>{usd(tax)}</span>
            </div>
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', backgroundColor: accent, color: '#fff', borderRadius: 10, padding: '12px 16px' }}>
              <span style={{ fontWeight: 600 }}>Total due</span>
              <span style={{ fontSize: 18, fontWeight: 700 }}>{usd(total)}</span>
            </div>
          </div>
        </div>

        {/* notes */}
        <div style={{ marginTop: 40, paddingTop: 16, borderTop: `1px solid ${line}`, display: 'flex', justifyContent: 'space-between', color: muted, fontSize: 9.5 }}>
          <span>Thank you for your business! Payment terms: net 30.</span>
          <span>northwind.studio</span>
        </div>
      </div>
    </div>
  );
}
