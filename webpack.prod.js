const { merge } = require('webpack-merge');
const { PurgeCSSPlugin } = require("purgecss-webpack-plugin");

const common = require('./webpack.common.js');
const glob = require("glob-all");

module.exports = merge(common, {
    mode: 'production',
    plugins: [...common.plugins, new PurgeCSSPlugin({
        keyframes: true,
        paths: glob.sync(['./client/**', './templates/**'], { nodir: true }),
        variables: true
    })]
});
