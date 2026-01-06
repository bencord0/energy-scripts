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
export { priceColors };
