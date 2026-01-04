const priceColors = {
    type: "threshold",
    domain: [7, 14, 27, 31],
    range: [
        "#0077be", // < 7: Less than Intelligent Octopus Off-Peak
        "#52be80", // 7-14: Less than Economy 7 Night Rate
        "#f1c40f", // 14-27: Less than the Flexible Rate (equivalent to the Ofgem Price Cap)
        "#e67e22", // 27-31: Less than the IOG Day Rate
        "#e74c3c"  // > 31: More than the Cosy Peak Rate
    ]
};
export { priceColors };
