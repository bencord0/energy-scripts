const priceColors = {
    type: "threshold",
    domain: [0, 7, 14, 26, 31],
    range: [
        "#00338a", // < 0: Negative energy prices!
        "#0077be", // 0-7: Less than Intelligent Octopus Off-Peak
        "#52be80", // 7-14: Less than Economy 7 Night Rate
        "#f1c40f", // 14-26: Less than the Flexible Rate (equivalent to the Ofgem Price Cap)
        "#e67e22", // 26-31: Less than the IOG Day Rate
        "#e74c3c"  // > 31: More than the Cosy Peak Rate
    ]
};

function addStripes(svg) {
    const ns = 'http://www.w3.org/2000/svg';
    let defs = svg.querySelector('defs');
    if (!defs) {
        defs = document.createElementNS(ns, 'defs');
        svg.insertBefore(defs, svg.firstChild);
    }
    if (!svg.querySelector('#stripes')) {
        const pattern = document.createElementNS(ns, 'pattern');
        pattern.setAttribute('id', 'stripes');
        pattern.setAttribute('patternUnits', 'userSpaceOnUse');
        pattern.setAttribute('width', '8');
        pattern.setAttribute('height', '8');

        // Transparent background so underlying tint shows through
        const bg = document.createElementNS(ns, 'rect');
        bg.setAttribute('width', '8');
        bg.setAttribute('height', '8');
        bg.setAttribute('fill', 'transparent');
        pattern.appendChild(bg);

        // Diagonal stripes
        const path = document.createElementNS(ns, 'path');
        path.setAttribute('d', 'M0,8 l8,-8 M-2,6 l4,-4 M6,10 l4,-4');
        path.setAttribute('stroke', '#000');
        path.setAttribute('stroke-opacity', '0.25');
        path.setAttribute('stroke-width', '1.0');
        path.setAttribute('fill', 'none');
        pattern.appendChild(path);

        defs.appendChild(pattern);
    }
}

export { priceColors, addStripes };
