const miniCssExtractPlugin = require('mini-css-extract-plugin');
const path = require('path');

module.exports = {
    entry: './client/app.js',
    module: {
        rules: [
            {
                test: /\.scss$/,
                use: [
                    miniCssExtractPlugin.loader,
                    'css-loader',
                    'sass-loader'
                ]
            }

        ]
    },
    output: {
        clean: true,
        filename: 'app.js',
        path: path.resolve(__dirname, 'dist')
    },
    plugins: [
        new miniCssExtractPlugin({
            filename: 'app.css'
        })
    ]
}
