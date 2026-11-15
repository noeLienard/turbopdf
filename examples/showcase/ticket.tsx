// A conference pass built with TurboPDF components: sizes in points, a native QR code, no HTML.
import { Document, Image, Page, QR, Text, View } from '../../src/components';

const night = '#0b1020';
const pink = '#f472b6';
const soft = '#94a3b8';

// Decorative rings, as an inline SVG (stays vector in the PDF)
const rings = `data:image/svg+xml,${encodeURIComponent(
  `<svg xmlns="http://www.w3.org/2000/svg" width="240" height="240" viewBox="0 0 240 240" fill="none">
    <circle cx="120" cy="120" r="118" stroke="#6366f1" stroke-width="2"/>
    <circle cx="120" cy="120" r="88" stroke="#a855f7" stroke-width="2"/>
    <circle cx="120" cy="120" r="58" stroke="${pink}" stroke-width="2"/>
    <circle cx="120" cy="120" r="28" fill="${pink}"/>
  </svg>`,
)}`;

const Field = ({ label, value }: { label: string; value: string }) => (
  <View style={{ gap: 3 }}>
    <Text style={{ fontSize: 7, fontWeight: 600, color: soft }}>{label}</Text>
    <Text style={{ fontSize: 11, fontWeight: 600, color: '#ffffff' }}>{value}</Text>
  </View>
);

export default function Ticket() {
  return (
    <Document title="Aurora Dev Summit 2027 — Conference pass" lang="en">
      <Page size={{ width: 640, height: 240 }} margin={0}>
        <View style={{ flexDirection: 'row', height: 240 }}>
          {/* main part */}
          <View style={{ flexGrow: 1, backgroundColor: night, padding: 28, justifyContent: 'space-between' }}>
            <Image src={rings} alt="" style={{ position: 'absolute', top: 24, right: 24, width: 140, height: 140 }} />
            <View style={{ gap: 6 }}>
              <Text style={{ fontSize: 8, fontWeight: 700, color: pink }}>AURORA DEV SUMMIT · SAN FRANCISCO</Text>
              <Text style={{ fontSize: 30, fontWeight: 700, color: '#ffffff' }}>Conference Pass</Text>
              <Text style={{ fontSize: 10, color: soft }}>Three days of systems, Rust and AI engineering.</Text>
            </View>
            <View style={{ flexDirection: 'row', gap: 26 }}>
              <Field label="ATTENDEE" value="Alex Morgan" />
              <Field label="DATES" value="Mar 12–14, 2027" />
              <Field label="ACCESS" value="All tracks + workshops" />
            </View>
          </View>
          {/* stub */}
          <View style={{ width: 168, backgroundColor: '#fdf2f8', padding: 16, alignItems: 'center', justifyContent: 'center', gap: 8 }}>
            <Text style={{ fontSize: 8, fontWeight: 700, color: '#be185d' }}>ADMIT ONE</Text>
            <QR value="https://aurora.dev/pass/AUR-000418" size={104} color={night} background="#fdf2f8" ecc="Q" alt="Pass code AUR-000418" />
            <Text style={{ fontSize: 9, fontWeight: 600, color: night }}>#AUR-000418</Text>
          </View>
        </View>
      </Page>
    </Document>
  );
}
