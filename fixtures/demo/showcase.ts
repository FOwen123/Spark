// The showcase design used for README screenshots: a fictional trip-planning app, written as
// HTML with inline styles, the way agents write designs. All content is made up.

export const LANDING = {
  name: "Atlas — Landing",
  styles: { width: "1280px", height: "fit-content", background: "#fafaf9" },
  sections: [
    `<div layer-name="Nav" style="display:flex;align-items:center;justify-content:space-between;padding:24px 64px">
      <div layer-name="Logo" style="display:flex;align-items:center;gap:10px">
        <svg width="28" height="28" viewBox="0 0 28 28"><circle cx="14" cy="14" r="13" fill="#0c0a09"/><path d="M8 18 L14 7 L20 18 Z" fill="#fafaf9"/></svg>
        <span style="font-size:20px;font-weight:600;letter-spacing:-0.02em;color:#0c0a09">Atlas</span>
      </div>
      <div layer-name="Links" style="display:flex;gap:32px;font-size:15px;color:#57534e"><span>Trips</span><span>Guides</span><span>Pricing</span></div>
      <div layer-name="Sign in" style="padding:10px 18px;border-radius:999px;background:#0c0a09;color:#fafaf9;font-size:14px;font-weight:500">Start planning</div>
    </div>`,
    `<div layer-name="Hero" style="display:flex;flex-direction:column;align-items:center;gap:20px;padding:72px 64px 48px;text-align:center">
      <span style="padding:6px 12px;border-radius:999px;border:1px solid #d6d3d1;font-size:13px;color:#57534e">New · Shared itineraries</span>
      <h1 style="font-size:72px;line-height:1.02;letter-spacing:-0.04em;margin:0;color:#0c0a09;max-width:880px">Plan the trip together, not in a group chat</h1>
      <p style="font-size:19px;line-height:1.55;color:#57534e;margin:0;max-width:620px">Atlas keeps flights, stays and ideas in one shared map, so everyone sees the same plan.</p>
      <div style="display:flex;gap:12px;margin-top:8px">
        <div style="padding:14px 22px;border-radius:12px;background:#0c0a09;color:#fafaf9;font-size:15px;font-weight:500">Create a trip</div>
        <div style="padding:14px 22px;border-radius:12px;border:1px solid #d6d3d1;color:#0c0a09;font-size:15px;font-weight:500">See an example</div>
      </div>
    </div>`,
    `<div layer-name="Preview" style="display:flex;gap:20px;padding:24px 64px 64px">
      <div layer-name="Map card" style="flex:2;height:360px;border-radius:24px;background:linear-gradient(140deg,#d6e4dc 0%,#a8c5b5 45%,#6f9a87 100%);position:relative;overflow:hidden">
        <div style="position:absolute;left:28px;top:28px;padding:10px 14px;border-radius:12px;background:#fafaf9;font-size:14px;color:#0c0a09;box-shadow:0 8px 24px rgb(0 0 0 / 0.12)">Lisbon → Porto · 6 days</div>
        <div style="position:absolute;left:180px;top:150px;width:18px;height:18px;border-radius:999px;background:#0c0a09;border:4px solid #fafaf9"></div>
        <div style="position:absolute;left:420px;top:90px;width:18px;height:18px;border-radius:999px;background:#0c0a09;border:4px solid #fafaf9"></div>
        <div style="position:absolute;left:300px;top:250px;width:18px;height:18px;border-radius:999px;background:#0c0a09;border:4px solid #fafaf9"></div>
      </div>
      <div layer-name="Day list" style="flex:1;display:flex;flex-direction:column;gap:12px">
        <div style="padding:18px;border-radius:18px;background:#fff;border:1px solid #e7e5e4"><div style="font-size:13px;color:#78716c">Day 1</div><div style="font-size:17px;font-weight:600;color:#0c0a09;margin-top:4px">Alfama walk and dinner</div></div>
        <div style="padding:18px;border-radius:18px;background:#fff;border:1px solid #e7e5e4"><div style="font-size:13px;color:#78716c">Day 2</div><div style="font-size:17px;font-weight:600;color:#0c0a09;margin-top:4px">Sintra by train</div></div>
        <div style="padding:18px;border-radius:18px;background:#0c0a09;color:#fafaf9"><div style="font-size:13px;opacity:0.7">Day 3</div><div style="font-size:17px;font-weight:600;margin-top:4px">Coast drive to Porto</div></div>
      </div>
    </div>`,
  ],
};

export const MOBILE = {
  name: "Atlas — Trip",
  styles: { width: "390px", height: "844px", background: "#0c0a09" },
  sections: [
    `<div layer-name="Status bar" style="display:flex;align-items:center;justify-content:space-between;height:54px;padding:0 30px;color:#fafaf9;font-size:16px;font-weight:600"><span>9:41</span><span style="letter-spacing:2px">●●●</span></div>`,
    `<div layer-name="Header" style="display:flex;flex-direction:column;gap:6px;padding:16px 24px">
      <span style="font-size:14px;color:#a8a29e">Your trip</span>
      <span style="font-size:32px;font-weight:600;letter-spacing:-0.03em;color:#fafaf9">Lisbon → Porto</span>
      <span style="font-size:15px;color:#a8a29e">May 4 – 10 · 4 travellers</span>
    </div>`,
    `<div layer-name="Days" style="display:flex;flex-direction:column;gap:12px;padding:12px 24px">
      <div style="display:flex;gap:14px;align-items:center;padding:16px;border-radius:18px;background:#1c1917"><div style="width:44px;height:44px;border-radius:12px;background:linear-gradient(135deg,#a8c5b5,#6f9a87)"></div><div style="display:flex;flex-direction:column;gap:2px"><span style="font-size:16px;font-weight:600;color:#fafaf9">Alfama walk</span><span style="font-size:13px;color:#a8a29e">Day 1 · 3 stops</span></div></div>
      <div style="display:flex;gap:14px;align-items:center;padding:16px;border-radius:18px;background:#1c1917"><div style="width:44px;height:44px;border-radius:12px;background:linear-gradient(135deg,#e7d7c1,#b9976b)"></div><div style="display:flex;flex-direction:column;gap:2px"><span style="font-size:16px;font-weight:600;color:#fafaf9">Sintra by train</span><span style="font-size:13px;color:#a8a29e">Day 2 · 2 stops</span></div></div>
      <div style="display:flex;gap:14px;align-items:center;padding:16px;border-radius:18px;background:#1c1917"><div style="width:44px;height:44px;border-radius:12px;background:linear-gradient(135deg,#c7d2fe,#6366f1)"></div><div style="display:flex;flex-direction:column;gap:2px"><span style="font-size:16px;font-weight:600;color:#fafaf9">Coast drive</span><span style="font-size:13px;color:#a8a29e">Day 3 · 4 stops</span></div></div>
    </div>`,
    `<div layer-name="Add button" style="margin:20px 24px 0;padding:16px;border-radius:16px;background:#fafaf9;color:#0c0a09;font-size:16px;font-weight:600;text-align:center">Add a day</div>`,
  ],
};
