// Vẽ HÌNH 3D (khối chóp/lăng trụ tuỳ ý, hoặc hình khối cơ bản: cầu/trụ/nón/
// hộp/xuyến) khi AI trả lời bằng khối mã ```3d — dùng cho hình học không
// gian (VD "khối chóp S.ABCD"). Xem rule liên quan trong SYSTEM_PROMPT
// (ai.rs).
//
// KHÁC HẲN mọi khối trước đó về RỦI RO BẢO MẬT cần cân nhắc: Three.js vốn là
// 1 API LẬP TRÌNH (viết code JS thật để dựng scene), không phải 1 định dạng
// khai báo thuần seperti Mermaid/ECharts/function-plot. TUYỆT ĐỐI KHÔNG để AI
// tự viết code Three.js rồi CHẠY THẲNG đoạn code đó — đó là THỰC THI MÃ JS
// TUỲ Ý do AI sinh ra, rủi ro bảo mật thật sự (khác hẳn các khối kia, chỉ là
// dữ liệu/markup đã sanitize, không có khả năng chạy lệnh gì). Ở ĐÂY, AI CHỈ
// được khai báo DỮ LIỆU THUẦN SỐ (toạ độ đỉnh, chỉ số mặt, loại khối cơ bản +
// kích thước) qua JSON — CODE (file này) mới là nơi DUY NHẤT gọi API
// Three.js thật để dựng scene, an toàn 100% vì input chỉ có thể là số/chuỗi
// ngắn, không có cách nào nhúng lệnh thực thi qua đường JSON.
import type { Action } from "svelte/action";
// `import type` — CHỈ lấy kiểu dữ liệu cho TypeScript kiểm tra lúc build,
// KHÔNG kéo theo module `three` thật vào bundle (dynamic import thật sự nằm
// trong `loadThree()` bên dưới) — biến `THREE` cục bộ trong các hàm dựng
// scene là 1 GIÁ TRỊ (namespace import trả về từ `ensureThree()`), không tự
// dùng được làm TÊN KIỂU (type) — cần import type riêng này cho các chỗ cần
// khai báo kiểu biến (VD `geometry` trong `buildScene`).
import type * as ThreeTypes from "three";
import { saveBytesAs } from "./exportImage";

let threePromise: ReturnType<typeof loadThree> | null = null;

async function loadThree() {
  const [THREE, controlsMod, css2dMod] = await Promise.all([
    import("three"),
    import("three/addons/controls/OrbitControls.js"),
    import("three/addons/renderers/CSS2DRenderer.js"),
  ]);
  return { THREE, OrbitControls: controlsMod.OrbitControls, CSS2DRenderer: css2dMod.CSS2DRenderer, CSS2DObject: css2dMod.CSS2DObject };
}

function ensureThree() {
  if (!threePromise) threePromise = loadThree();
  return threePromise;
}

// ── Cú pháp JSON (xem PROMPT_3D trong ai.rs) ────────────────────────────────

interface PrimitiveSpec {
  type: "sphere" | "cylinder" | "cone" | "box" | "torus";
  position?: [number, number, number];
  color?: string;
  wireframe?: boolean;
  // Kích thước — tuỳ loại khối chỉ vài trường dưới đây có nghĩa, thiếu thì
  // dùng mặc định hợp lý (bán kính 1, chiều cao 2...).
  radius?: number;
  height?: number;
  width?: number;
  depth?: number;
  tube?: number;
}

interface LabelSpec {
  vertex: number;
  text: string;
}

interface AtomSpec {
  /** Ký hiệu nguyên tố (H, C, O, N, Cl...) — dùng để tự tra màu/bán kính \
   * chuẩn CPK bên dưới, KHÔNG cần AI tự chọn màu. */
  element: string;
  position: [number, number, number];
}

interface BondSpec {
  /** Chỉ số nguyên tử (trong `molecule.atoms`) ở 2 đầu liên kết. */
  from: number;
  to: number;
  /** Bậc liên kết — 1 (đơn, mặc định), 2 (đôi), 3 (ba). Liên kết đôi/ba tự
   * vẽ thành nhiều trụ song song thay vì 1 trụ dày. */
  order?: 1 | 2 | 3;
}

interface MoleculeSpec {
  atoms: AtomSpec[];
  bonds?: BondSpec[];
}

interface Scene3dSpec {
  objects?: PrimitiveSpec[];
  /** Toạ độ TỪNG ĐỈNH của khối đa diện tuỳ ý (khối chóp/lăng trụ...). */
  vertices?: [number, number, number][];
  /** Mỗi mặt là 1 mảng CHỈ SỐ đỉnh (trong `vertices`), theo thứ tự quanh
   * biên mặt đó — hỗ trợ mặt tam giác LẪN đa giác (tự tam giác hoá kiểu
   * "quạt" từ đỉnh đầu tiên, đủ dùng cho mặt lồi thường gặp ở khối chóp/lăng
   * trụ, KHÔNG đúng cho mặt lõm — hiếm gặp trong đề hình học phổ thông). */
  faces?: number[][];
  labels?: LabelSpec[];
  color?: string;
  molecule?: MoleculeSpec;
}

function parseScene3dSpec(source: string): Scene3dSpec | null {
  try {
    const spec = JSON.parse(source);
    if (!spec || typeof spec !== "object" || Array.isArray(spec)) return null;
    // Phải có ÍT NHẤT 1 trong 3 cách khai báo hình — JSON hợp lệ nhưng rỗng
    // hoàn toàn không có gì để vẽ.
    if (!Array.isArray(spec.objects) && !Array.isArray(spec.vertices) && !Array.isArray(spec.molecule?.atoms)) return null;
    return spec as Scene3dSpec;
  } catch {
    return null;
  }
}

const DEFAULT_COLORS = ["#7c5cff", "#22c55e", "#f59e0b", "#ef4444", "#06b6d4"];

/** Bảng màu + bán kính CPK (quy ước hiển thị phân tử chuẩn, dùng chung ở mọi
 * phần mềm hoá học: PyMOL, 3Dmol.js, Jmol...) — AI KHÔNG cần tự chọn màu cho
 * từng nguyên tử, chỉ cần ghi đúng ký hiệu nguyên tố, tra bảng này ra ngay
 * đúng màu/kích thước chuẩn mà ai học hoá cũng quen mắt. Bán kính đã CO NHỎ
 * hơn bán kính van der Waals thật (chỉ giữ tỉ lệ tương đối giữa các nguyên
 * tố) — dùng bán kính thật sẽ khiến nguyên tử chồng khít lên nhau, không
 * thấy rõ liên kết ở giữa. */
const CPK_TABLE: Record<string, { color: string; radius: number }> = {
  H: { color: "#ffffff", radius: 0.32 },
  C: { color: "#4d4d4d", radius: 0.5 },
  N: { color: "#3050f8", radius: 0.48 },
  O: { color: "#ff0d0d", radius: 0.48 },
  F: { color: "#90e050", radius: 0.42 },
  CL: { color: "#1ff01f", radius: 0.58 },
  BR: { color: "#a62929", radius: 0.62 },
  I: { color: "#940094", radius: 0.68 },
  S: { color: "#ffff30", radius: 0.58 },
  P: { color: "#ff8000", radius: 0.56 },
  NA: { color: "#ab5cf2", radius: 0.6 },
  K: { color: "#8f40d4", radius: 0.68 },
  CA: { color: "#3dff00", radius: 0.66 },
  FE: { color: "#e06633", radius: 0.56 },
  MG: { color: "#8aff00", radius: 0.56 },
  ZN: { color: "#7d80b0", radius: 0.54 },
};
const CPK_DEFAULT = { color: "#e91e8c", radius: 0.5 };

function cpkOf(element: string): { color: string; radius: number } {
  return CPK_TABLE[element.trim().toUpperCase()] ?? CPK_DEFAULT;
}

/** Dựng TOÀN BỘ scene (camera/ánh sáng/lưới sàn/vật thể/nhãn) từ 1 `spec` —
 * dùng chung cho cả bản nhúng nhỏ trong chat lẫn modal phóng to, chỉ khác
 * kích thước canvas truyền vào. Trả về hàm `render()` để gọi lại mỗi khi
 * OrbitControls đổi góc nhìn (render theo sự kiện, KHÔNG chạy vòng lặp
 * animation liên tục — tránh phải tự dọn dẹp requestAnimationFrame khi bong
 * bóng chat bị gỡ khỏi DOM, xem thêm ghi chú ở `renderScene3dBlocks`). */
async function buildScene(container: HTMLElement, spec: Scene3dSpec, width: number, height: number) {
  const { THREE, OrbitControls, CSS2DRenderer, CSS2DObject } = await ensureThree();

  const scene = new THREE.Scene();
  scene.background = new THREE.Color("#ffffff");

  const camera = new THREE.PerspectiveCamera(45, width / height, 0.1, 1000);
  camera.position.set(4, 3.5, 6);

  const renderer = new THREE.WebGLRenderer({ antialias: true, preserveDrawingBuffer: true });
  renderer.setSize(width, height);
  renderer.setPixelRatio(Math.min(2, window.devicePixelRatio || 1));
  container.appendChild(renderer.domElement);

  // CSS2DRenderer — vẽ nhãn (tên đỉnh) bằng <div> HTML thật, TỰ ĐỘNG chiếu
  // đúng vị trí 2D tương ứng với toạ độ 3D mỗi khi camera xoay/zoom (khác
  // hẳn vẽ text bằng texture/canvas — HTML luôn nét dù zoom cỡ nào). Đặt
  // pointer-events:none — không được chặn thao tác kéo xoay của OrbitControls.
  const labelRenderer = new CSS2DRenderer();
  labelRenderer.setSize(width, height);
  labelRenderer.domElement.style.cssText = "position:absolute;top:0;left:0;pointer-events:none;";
  container.appendChild(labelRenderer.domElement);

  scene.add(new THREE.AmbientLight(0xffffff, 0.7));
  const dirLight = new THREE.DirectionalLight(0xffffff, 0.8);
  dirLight.position.set(5, 8, 6);
  scene.add(dirLight);

  // Lưới sàn mờ — cho người xem CẢM NHẬN được góc nhìn/tỉ lệ không gian,
  // không có thì hình khối trôi lơ lửng khó hình dung xoay theo hướng nào.
  const grid = new THREE.GridHelper(10, 10, 0xdddddd, 0xeeeeee);
  scene.add(grid);

  const group = new THREE.Group();
  scene.add(group);

  // ── Hình khối cơ bản ────────────────────────────────────────────────────
  (spec.objects ?? []).forEach((obj, i) => {
    const color = obj.color || DEFAULT_COLORS[i % DEFAULT_COLORS.length];
    const material = new THREE.MeshStandardMaterial({ color, wireframe: !!obj.wireframe });
    let geometry: ThreeTypes.BufferGeometry<ThreeTypes.NormalBufferAttributes> | null = null;
    switch (obj.type) {
      case "sphere":
        geometry = new THREE.SphereGeometry(obj.radius ?? 1, 32, 24);
        break;
      case "cylinder":
        geometry = new THREE.CylinderGeometry(obj.radius ?? 1, obj.radius ?? 1, obj.height ?? 2, 32);
        break;
      case "cone":
        geometry = new THREE.ConeGeometry(obj.radius ?? 1, obj.height ?? 2, 32);
        break;
      case "box":
        geometry = new THREE.BoxGeometry(obj.width ?? 1, obj.height ?? 1, obj.depth ?? 1);
        break;
      case "torus":
        geometry = new THREE.TorusGeometry(obj.radius ?? 1, obj.tube ?? 0.35, 16, 48);
        break;
    }
    if (!geometry) return;
    const mesh = new THREE.Mesh(geometry, material);
    if (obj.position) mesh.position.set(...obj.position);
    group.add(mesh);
    // Viền cạnh đậm — CHỈ cho khối có cạnh SẮC thật (hộp) — sphere/cylinder/
    // cone/torus là mặt CONG, chia lưới nhiều đoạn (32+) để mượt, nên
    // EdgesGeometry sẽ đánh dấu gần NHƯ MỌI đường lưới phân đoạn thành
    // "cạnh" (góc giữa 2 tam giác kề luôn đủ lớn ở mặt cong) — kết quả bị phủ
    // kín lưới ô vuông dày đặc trông như quả bóng lưới, KHÔNG phải viền làm
    // rõ hình khối (lỗi thực tế đã gặp, xem ảnh người dùng gửi: mặt cầu bị
    // phủ lưới đen dày đặc).
    if (obj.type === "box") {
      const edges = new THREE.LineSegments(new THREE.EdgesGeometry(geometry), new THREE.LineBasicMaterial({ color: 0x333333 }));
      edges.position.copy(mesh.position);
      group.add(edges);
    }
  });

  // ── Đa diện tuỳ ý (khối chóp/lăng trụ...) từ toạ độ đỉnh + mặt ──────────
  if (spec.vertices && spec.vertices.length > 0) {
    const positions: number[] = [];
    for (const face of spec.faces ?? []) {
      // Tam giác hoá kiểu "quạt": đỉnh đầu tiên của mặt nối với TỪNG cặp
      // đỉnh liền kề còn lại — đúng với mọi mặt LỒI (tam giác, tứ giác lồi,
      // ngũ giác lồi...), đủ dùng cho hầu hết khối chóp/lăng trụ trong đề.
      for (let k = 1; k < face.length - 1; k++) {
        for (const idx of [face[0], face[k], face[k + 1]]) {
          const v = spec.vertices[idx];
          if (v) positions.push(v[0], v[1], v[2]);
        }
      }
    }
    if (positions.length > 0) {
      const geometry = new THREE.BufferGeometry();
      geometry.setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
      geometry.computeVertexNormals();
      const material = new THREE.MeshStandardMaterial({
        color: spec.color || DEFAULT_COLORS[0],
        side: THREE.DoubleSide,
        transparent: true,
        opacity: 0.85,
      });
      const mesh = new THREE.Mesh(geometry, material);
      group.add(mesh);
      const edges = new THREE.LineSegments(new THREE.EdgesGeometry(geometry), new THREE.LineBasicMaterial({ color: 0x333333 }));
      group.add(edges);
    }

    for (const label of spec.labels ?? []) {
      const v = spec.vertices[label.vertex];
      if (!v) continue;
      const div = document.createElement("div");
      div.textContent = label.text;
      div.style.cssText = "font:700 13px sans-serif;color:#111;background:#fff;padding:1px 5px;border-radius:4px;border:1px solid #ccc;";
      const labelObj = new CSS2DObject(div);
      labelObj.position.set(v[0], v[1], v[2]);
      group.add(labelObj);
    }
  }

  // ── Phân tử (nguyên tử + liên kết) ──────────────────────────────────────
  // Nguyên tử: hình cầu TÔ ĐÚNG màu/kích thước chuẩn CPK theo ký hiệu nguyên
  // tố (xem `cpkOf`) — KHÔNG lấy màu từ `DEFAULT_COLORS` như khối cơ bản
  // (lỗi thực tế đã gặp trước khi có schema riêng này: AI phải "chế" phân tử
  // bằng khối cầu thường, ra màu tuỳ tiện không đúng quy ước hoá học, viền
  // wireframe dày đặc do lỗi edges ở trên). Nguyên tử là mặt CONG nên KHÔNG
  // thêm viền cạnh — cùng lý do đã sửa ở khối cơ bản.
  if (spec.molecule && spec.molecule.atoms.length > 0) {
    const atoms = spec.molecule.atoms;
    for (const atom of atoms) {
      const { color, radius } = cpkOf(atom.element);
      const mesh = new THREE.Mesh(
        new THREE.SphereGeometry(radius, 24, 18),
        new THREE.MeshStandardMaterial({ color }),
      );
      mesh.position.set(...atom.position);
      group.add(mesh);
    }

    // Liên kết: hình trụ nối tâm 2 nguyên tử — trụ mặc định dựng THẲNG ĐỨNG
    // (dọc trục Y), phải tự xoay bằng quaternion cho khớp đúng hướng vector
    // nối 2 nguyên tử (kỹ thuật kinh điển: `setFromUnitVectors` từ trục Y
    // chuẩn sang hướng liên kết thật). Liên kết đôi/ba tách thành 2-3 trụ
    // MẢNH đặt song song, lệch sang 2 bên trục liên kết — đúng quy ước vẽ
    // hoá học phổ thông, không phải 1 trụ dày trơn.
    const bondMaterial = new THREE.MeshStandardMaterial({ color: "#888888" });
    for (const bond of spec.molecule.bonds ?? []) {
      const a = atoms[bond.from]?.position;
      const b = atoms[bond.to]?.position;
      if (!a || !b) continue;
      const start = new THREE.Vector3(...a);
      const end = new THREE.Vector3(...b);
      const bondVec = new THREE.Vector3().subVectors(end, start);
      const length = bondVec.length();
      if (length < 1e-6) continue;
      const mid = new THREE.Vector3().addVectors(start, end).multiplyScalar(0.5);
      const quaternion = new THREE.Quaternion().setFromUnitVectors(new THREE.Vector3(0, 1, 0), bondVec.clone().normalize());

      // Trục vuông góc với liên kết (để lệch trụ song song sang 2 bên) —
      // chọn 1 trục tạm bất kỳ không song song với liên kết rồi lấy tích có
      // hướng, đảm bảo luôn ra 1 vector vuông góc hợp lệ dù liên kết hướng
      // theo trục nào.
      const arbitrary = Math.abs(bondVec.y) > 0.9 ? new THREE.Vector3(1, 0, 0) : new THREE.Vector3(0, 1, 0);
      const perp = new THREE.Vector3().crossVectors(bondVec, arbitrary).normalize();

      const order = bond.order ?? 1;
      const bondRadius = order > 1 ? 0.05 : 0.09;
      const offsets = order === 3 ? [-0.16, 0, 0.16] : order === 2 ? [-0.1, 0.1] : [0];
      for (const off of offsets) {
        const cyl = new THREE.Mesh(new THREE.CylinderGeometry(bondRadius, bondRadius, length, 12), bondMaterial);
        cyl.position.copy(mid).addScaledVector(perp, off);
        cyl.quaternion.copy(quaternion);
        group.add(cyl);
      }
    }
  }

  // Canh giữa + lùi camera đủ xa để thấy TRỌN hình, bất kể toạ độ AI đưa ra
  // lớn/nhỏ cỡ nào — tự đo bounding box thay vì đoán 1 khoảng cách cố định.
  const box = new THREE.Box3().setFromObject(group);
  if (!box.isEmpty()) {
    const center = box.getCenter(new THREE.Vector3());
    const size = box.getSize(new THREE.Vector3());
    const maxDim = Math.max(size.x, size.y, size.z, 0.001);
    group.position.sub(center); // đưa hình về gốc toạ độ cho lưới sàn làm chuẩn
    camera.position.set(maxDim * 1.1, maxDim * 0.9, maxDim * 1.4);
    camera.lookAt(0, 0, 0);
  }

  const controls = new OrbitControls(camera, labelRenderer.domElement);
  controls.target.set(0, 0, 0);
  controls.enableDamping = false; // không cần vòng lặp animation liên tục — xem giải thích render theo sự kiện

  function render() {
    renderer.render(scene, camera);
    labelRenderer.render(scene, camera);
  }
  controls.addEventListener("change", render);
  render();

  return { renderer, camera, controls, render };
}

async function renderScene3dBlocks(container: HTMLElement): Promise<void> {
  const codeEls = Array.from(container.querySelectorAll<HTMLElement>("code.language-3d"));
  if (codeEls.length === 0) return;

  for (const codeEl of codeEls) {
    const pre = codeEl.closest("pre");
    if (!pre || pre.dataset.scene3dTried === "1") continue;
    pre.dataset.scene3dTried = "1";

    const source = codeEl.textContent ?? "";
    const spec = parseScene3dSpec(source);
    // JSON hỏng/rỗng (AI hallucinate) — giữ nguyên khối code gốc.
    if (!spec) continue;

    try {
      const wrapper = document.createElement("div");
      wrapper.className = "scene3d";
      wrapper.style.cssText =
        "position:relative;border-radius:10px;overflow:hidden;background:#fff;" +
        "border:1px solid #e2e2e2;display:inline-block;max-width:100%;";

      const stageEl = document.createElement("div");
      const width = Math.max(240, Math.min(420, container.clientWidth || 420));
      const height = 280;
      stageEl.style.cssText = `position:relative;width:${width}px;height:${height}px;max-width:100%;`;
      wrapper.appendChild(stageEl);

      await buildScene(stageEl, spec, width, height);

      const zoomBtn = document.createElement("button");
      zoomBtn.type = "button";
      zoomBtn.title = "Phóng to (kéo để xoay, cuộn để zoom)";
      zoomBtn.setAttribute("aria-label", "Phóng to hình 3D");
      zoomBtn.innerHTML = ZOOM_ICON_SVG;
      zoomBtn.style.cssText =
        "position:absolute;top:6px;right:6px;width:26px;height:26px;border-radius:6px;" +
        "border:1px solid var(--color-border);background:var(--color-bg-elevated);" +
        "color:var(--color-text-muted);cursor:pointer;display:flex;align-items:center;" +
        "justify-content:center;padding:0;";
      zoomBtn.addEventListener("click", (e) => {
        e.stopPropagation();
        openScene3dZoomModal(spec);
      });
      wrapper.appendChild(zoomBtn);

      const downloadBtn = document.createElement("button");
      downloadBtn.type = "button";
      downloadBtn.title = "Tải ảnh PNG";
      downloadBtn.setAttribute("aria-label", "Tải ảnh PNG");
      downloadBtn.innerHTML = DOWNLOAD_ICON_SVG;
      downloadBtn.style.cssText =
        "position:absolute;top:34px;right:6px;width:26px;height:26px;border-radius:6px;" +
        "border:1px solid var(--color-border);background:var(--color-bg-elevated);" +
        "color:var(--color-text-muted);cursor:pointer;display:flex;align-items:center;" +
        "justify-content:center;padding:0;";
      downloadBtn.addEventListener("click", (e) => {
        e.stopPropagation();
        const canvas = stageEl.querySelector("canvas");
        if (!canvas) return;
        canvas.toBlob((blob) => {
          if (!blob) return;
          blob
            .arrayBuffer()
            .then((buf) => saveBytesAs(buf, "hinh-3d.png", [{ name: "Ảnh PNG", extensions: ["png"] }]))
            .catch((err) => console.warn("[snip-ai] Xuất PNG hình 3D thất bại:", err));
        }, "image/png");
      });
      wrapper.appendChild(downloadBtn);

      pre.replaceWith(wrapper);
    } catch (e) {
      console.warn("[snip-ai] Không vẽ được hình 3D, giữ nguyên code gốc:", e);
    }
  }
}

/** Action gắn vào container `.markdown-body` — cùng cách dùng với
 * `mermaidBlocks`/`plotBlocks`/`svgFigureBlocks`/`chartBlocks`. */
export const scene3dBlocks: Action<HTMLElement, unknown> = (node) => {
  renderScene3dBlocks(node);
  return {
    update() {
      renderScene3dBlocks(node);
    },
  };
};

// ── Phóng to (modal riêng) ───────────────────────────────────────────────

const ZOOM_ICON_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" ' +
  'stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">' +
  '<path d="M15 3h6v6"/><path d="M9 21H3v-6"/><path d="M21 3l-7 7"/><path d="M3 21l7-7"/></svg>';

const DOWNLOAD_ICON_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" ' +
  'stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">' +
  '<path d="M12 3v12"/><path d="m7 10 5 5 5-5"/><path d="M4 19h16"/></svg>';

function openScene3dZoomModal(spec: Scene3dSpec): void {
  const overlay = document.createElement("div");
  overlay.style.cssText =
    "position:fixed;inset:0;z-index:9999;background:rgba(0,0,0,.82);" +
    "display:flex;align-items:center;justify-content:center;overflow:hidden;padding:40px;";

  const card = document.createElement("div");
  card.style.cssText =
    "position:relative;background:#fff;border-radius:14px;padding:16px;max-width:100%;max-height:100%;" +
    "box-shadow:0 20px 60px rgba(0,0,0,.4);";
  overlay.appendChild(card);

  const stageEl = document.createElement("div");
  card.appendChild(stageEl);

  const closeBtn = document.createElement("button");
  closeBtn.type = "button";
  closeBtn.title = "Đóng (Esc)";
  closeBtn.setAttribute("aria-label", "Đóng");
  closeBtn.textContent = "×";
  closeBtn.style.cssText =
    "position:absolute;top:16px;right:16px;width:34px;height:34px;border-radius:8px;" +
    "border:1px solid rgba(255,255,255,.25);background:rgba(255,255,255,.1);color:#fff;" +
    "font-size:20px;line-height:1;cursor:pointer;display:flex;align-items:center;justify-content:center;z-index:1;";
  overlay.appendChild(closeBtn);

  function close() {
    document.removeEventListener("keydown", onKeydown);
    overlay.remove();
  }
  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") close();
  }
  closeBtn.addEventListener("click", close);
  overlay.addEventListener("click", (e) => {
    if (e.target === overlay) close();
  });
  document.addEventListener("keydown", onKeydown);

  document.body.appendChild(overlay);

  const width = Math.min(760, window.innerWidth - 120);
  const height = Math.min(540, window.innerHeight - 160);
  stageEl.style.cssText = `position:relative;width:${width}px;height:${height}px;`;
  buildScene(stageEl, spec, width, height).catch((err) => console.warn("[snip-ai] Không dựng được hình 3D phóng to:", err));
}
