const { merge } = require('webpack-merge');
const { PurgeCSSPlugin } = require("purgecss-webpack-plugin");

const common = require('./webpack.common.js');
const glob = require("glob");
const path = require('path');

module.exports = merge(common, {
    mode: 'production',
    plugins: [...common.plugins, new PurgeCSSPlugin({
        keyframes: true,
        paths: glob.sync(path.join(__dirname, 'templates') + '/**/*', { nodir: true }),
        variables: true
    })]
});
